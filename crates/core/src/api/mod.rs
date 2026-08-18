//! Local HTTPS + WSS API. Techstack §8 contract (UC-106).

mod auth;
mod ws;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::Json;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use homeai_common::{Config, Paths, TokenRecord};
use serde::{Deserialize, Serialize};
use tonic_health::server::HealthReporter;
use tracing::info;

use crate::bus::{core_event, Bus};
use crate::db::{now_ms, Db};
use crate::health::HealthState;
use crate::model::{Device, Room, Sensor};
use crate::Error;

use self::auth::{AdminAuth, AuthLimiter, ControlAuth, ReadAuth};

#[derive(Clone)]
struct ApiState {
    health: HealthState,
    paths: Paths,
    config: Config,
    db: Db,
    bus: Bus,
    limiter: AuthLimiter,
}

pub async fn serve(
    config: Config,
    paths: Paths,
    health: HealthState,
    db: Db,
    bus: Bus,
) -> Result<(), Error> {
    let addr = config.api_addr()?;
    let tls = RustlsConfig::from_pem_file(paths.tls_cert(), paths.tls_key())
        .await
        .map_err(|e| Error::Other(format!("api tls: {e}")))?;

    let app = router(ApiState {
        health,
        paths,
        config,
        db,
        bus,
        limiter: AuthLimiter::new(),
    });

    info!(%addr, "api listening (https)");
    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|e| Error::Other(format!("api serve: {e}")))?;
    Ok(())
}

fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/status", get(status_handler))
        .route("/api/v1/house", get(house_handler))
        .route(
            "/api/v1/rooms",
            get(rooms_handler).post(create_room_handler),
        )
        .route(
            "/api/v1/rooms/{id}",
            get(room_detail_handler).delete(delete_room_handler),
        )
        .route(
            "/api/v1/devices",
            get(devices_handler).post(create_device_handler),
        )
        .route("/api/v1/devices/{id}", delete(delete_device_handler))
        .route("/api/v1/devices/{id}/command", post(device_command_handler))
        .route(
            "/api/v1/sensors",
            get(sensors_handler).post(create_sensor_handler),
        )
        .route("/api/v1/presence", get(presence_handler))
        .route("/api/v1/voice/say", post(voice_say_handler))
        .route(
            "/api/v1/conversations/{id}/attachments",
            post(attachment_handler),
        )
        .route(
            "/api/v1/system/diagnostics/export",
            post(diagnostics_export_handler),
        )
        .route(
            "/api/v1/system/ownership-transfer",
            post(ownership_transfer_handler),
        )
        .route("/api/v1/system/factory-reset", post(factory_reset_handler))
        .route("/api/v1/system/update", post(system_update_handler))
        .route("/ws/events", get(ws::events_handler))
        .with_state(state)
}

async fn health_handler(State(state): State<ApiState>) -> Json<crate::health::HealthSnapshot> {
    Json(state.health.snapshot())
}

#[derive(Serialize)]
struct StatusBody {
    status: &'static str,
    llm: String,
    stt: String,
    tts: String,
    wake: String,
    token_id: String,
}

async fn status_handler(
    State(state): State<ApiState>,
    ReadAuth(rec): ReadAuth,
) -> Json<StatusBody> {
    Json(StatusBody {
        status: "ok",
        llm: state.config.llm.url.clone(),
        stt: state.config.stt.url.clone(),
        tts: state.config.tts.url.clone(),
        wake: state.config.wake.keyword.clone(),
        token_id: rec.id,
    })
}

async fn house_handler(
    State(state): State<ApiState>,
    ReadAuth(_): ReadAuth,
) -> Result<Json<crate::model::House>, (StatusCode, String)> {
    let house = state.db.load_house().map_err(db_err)?;
    Ok(Json(house))
}

async fn rooms_handler(
    State(state): State<ApiState>,
    ReadAuth(_): ReadAuth,
) -> Result<Json<Vec<Room>>, (StatusCode, String)> {
    let house = state.db.load_house().map_err(db_err)?;
    Ok(Json(house.rooms))
}

#[derive(Serialize)]
struct RoomDetail {
    #[serde(flatten)]
    room: Room,
    devices: Vec<Device>,
    sensors: Vec<Sensor>,
    presence: Vec<serde_json::Value>,
}

async fn room_detail_handler(
    State(state): State<ApiState>,
    ReadAuth(_): ReadAuth,
    Path(id): Path<String>,
) -> Result<Json<RoomDetail>, (StatusCode, String)> {
    let room = state
        .db
        .get_room(&id)
        .map_err(db_err)?
        .ok_or((StatusCode::NOT_FOUND, "room not found".into()))?;
    let house = state.db.load_house().map_err(db_err)?;
    let devices = house
        .devices
        .into_iter()
        .filter(|d| d.room_id == id)
        .collect();
    let sensors = house
        .sensors
        .into_iter()
        .filter(|s| s.room_id.as_deref() == Some(id.as_str()))
        .collect();
    Ok(Json(RoomDetail {
        room,
        devices,
        sensors,
        presence: Vec::new(),
    }))
}

#[derive(Deserialize)]
struct NewRoom {
    id: Option<String>,
    floor_id: String,
    name: String,
    kind: Option<String>,
}

async fn create_room_handler(
    State(state): State<ApiState>,
    ControlAuth(_): ControlAuth,
    Json(body): Json<NewRoom>,
) -> Result<(StatusCode, Json<Room>), (StatusCode, String)> {
    let room = Room {
        id: body
            .id
            .unwrap_or_else(|| format!("room-{}", crate::db::now_ms())),
        floor_id: body.floor_id,
        name: body.name,
        kind: body.kind.unwrap_or_else(|| "indoor".into()),
    };
    state.db.put_room(room.clone()).map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(room)))
}

async fn delete_room_handler(
    State(state): State<ApiState>,
    ControlAuth(_): ControlAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if state.db.delete_room(&id).map_err(db_err)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "room not found".into()))
    }
}

async fn devices_handler(
    State(state): State<ApiState>,
    ReadAuth(_): ReadAuth,
) -> Result<Json<Vec<Device>>, (StatusCode, String)> {
    let house = state.db.load_house().map_err(db_err)?;
    Ok(Json(house.devices))
}

#[derive(Deserialize)]
struct NewDevice {
    id: Option<String>,
    room_id: String,
    name: String,
    kind: Option<String>,
    protocol: Option<String>,
}

async fn create_device_handler(
    State(state): State<ApiState>,
    ControlAuth(_): ControlAuth,
    Json(body): Json<NewDevice>,
) -> Result<(StatusCode, Json<Device>), (StatusCode, String)> {
    if state.db.get_room(&body.room_id).map_err(db_err)?.is_none() {
        return Err((StatusCode::BAD_REQUEST, "room not found".into()));
    }
    let device = Device {
        id: body
            .id
            .unwrap_or_else(|| format!("device-{}", crate::db::now_ms())),
        room_id: body.room_id,
        name: body.name,
        kind: body.kind.unwrap_or_else(|| "unknown".into()),
        protocol: body.protocol,
    };
    state.db.put_device(device.clone()).map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(device)))
}

async fn delete_device_handler(
    State(state): State<ApiState>,
    ControlAuth(_): ControlAuth,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if state.db.delete_device(&id).map_err(db_err)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "device not found".into()))
    }
}

#[derive(Deserialize)]
struct DeviceCommandBody {
    action: String,
    #[serde(default)]
    params: serde_json::Value,
}

async fn device_command_handler(
    State(state): State<ApiState>,
    ControlAuth(rec): ControlAuth,
    Path(id): Path<String>,
    Json(body): Json<DeviceCommandBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let device = state
        .db
        .get_device(&id)
        .map_err(db_err)?
        .ok_or((StatusCode::NOT_FOUND, "device not found".into()))?;
    if body.action.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "action required".into()));
    }
    let payload = serde_json::json!({
        "device_id": device.id,
        "action": body.action,
        "params": body.params,
        "token_id": rec.id,
    });
    publish(&state.bus, "device.command", &rec, &device.room_id, payload)?;
    Ok(Json(serde_json::json!({
        "status": "accepted",
        "device_id": device.id,
        "action": body.action,
    })))
}

async fn sensors_handler(
    State(state): State<ApiState>,
    ReadAuth(_): ReadAuth,
) -> Result<Json<Vec<Sensor>>, (StatusCode, String)> {
    let house = state.db.load_house().map_err(db_err)?;
    Ok(Json(house.sensors))
}

#[derive(Deserialize)]
struct NewSensor {
    id: Option<String>,
    room_id: Option<String>,
    device_id: Option<String>,
    kind: String,
}

async fn create_sensor_handler(
    State(state): State<ApiState>,
    ControlAuth(_): ControlAuth,
    Json(body): Json<NewSensor>,
) -> Result<(StatusCode, Json<Sensor>), (StatusCode, String)> {
    let sensor = Sensor {
        id: body
            .id
            .unwrap_or_else(|| format!("sensor-{}", crate::db::now_ms())),
        room_id: body.room_id,
        device_id: body.device_id,
        kind: body.kind,
    };
    state.db.put_sensor(sensor.clone()).map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(sensor)))
}

#[derive(Serialize)]
struct PresenceBody {
    people: Vec<serde_json::Value>,
    rooms: Vec<serde_json::Value>,
}

async fn presence_handler(ReadAuth(_): ReadAuth) -> Json<PresenceBody> {
    Json(PresenceBody {
        people: Vec::new(),
        rooms: Vec::new(),
    })
}

#[derive(Deserialize)]
struct VoiceSayBody {
    room_id: String,
    text: String,
}

async fn voice_say_handler(
    State(state): State<ApiState>,
    ControlAuth(rec): ControlAuth,
    Json(body): Json<VoiceSayBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "text required".into()));
    }
    if state.db.get_room(&body.room_id).map_err(db_err)?.is_none() {
        return Err((StatusCode::BAD_REQUEST, "room not found".into()));
    }
    info!(
        token_id = %rec.id,
        room_id = %body.room_id,
        chars = body.text.len(),
        "voice.say"
    );
    let payload = serde_json::json!({
        "room_id": body.room_id,
        "text": body.text,
        "token_id": rec.id,
    });
    publish(&state.bus, "voice.say", &rec, &body.room_id, payload)?;
    Ok(Json(serde_json::json!({
        "status": "accepted",
        "room_id": body.room_id,
    })))
}

async fn attachment_handler(
    ControlAuth(_): ControlAuth,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    // UC-244 owns validation, VLM, and retention. Surface exists so auth is real.
    Json(serde_json::json!({
        "status": "no_active_request",
        "conversation_id": id,
    }))
}

async fn diagnostics_export_handler(AdminAuth(rec): AdminAuth) -> Json<serde_json::Value> {
    info!(token_id = %rec.id, "system.diagnostics.export");
    Json(serde_json::json!({ "status": "pending_consent" }))
}

async fn ownership_transfer_handler(AdminAuth(rec): AdminAuth) -> Json<serde_json::Value> {
    info!(token_id = %rec.id, "system.ownership-transfer");
    Json(serde_json::json!({ "status": "pending_confirmation" }))
}

async fn factory_reset_handler(AdminAuth(rec): AdminAuth) -> Json<serde_json::Value> {
    info!(token_id = %rec.id, "system.factory-reset");
    Json(serde_json::json!({ "status": "pending_confirmation" }))
}

async fn system_update_handler(AdminAuth(rec): AdminAuth) -> Json<serde_json::Value> {
    info!(token_id = %rec.id, "system.update");
    Json(serde_json::json!({ "status": "no_bundle" }))
}

fn publish(
    bus: &Bus,
    event_type: &str,
    rec: &TokenRecord,
    room_id: &str,
    payload: serde_json::Value,
) -> Result<(), (StatusCode, String)> {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let unique = format!("{}-{}", now_ms(), SEQ.fetch_add(1, Ordering::Relaxed));
    let mut event = core_event(
        event_type,
        &format!("token:{}", rec.id),
        &unique,
        payload.to_string().into_bytes(),
    );
    event.room_id = room_id.to_string();
    bus.publish(event)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn db_err(err: crate::db::DbError) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
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

enum HealthSvcMarker {}

impl tonic::server::NamedService for HealthSvcMarker {
    const NAME: &'static str = "homeai.core.Health";
}

pub fn ephemeral_localhost() -> std::io::Result<SocketAddr> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr()
}
