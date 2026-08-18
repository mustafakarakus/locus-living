//! Local HTTPS API (house CRUD + health/status). Full contract lock-down is later.

use std::net::SocketAddr;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::{delete, get};
use axum::Json;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use homeai_common::{AuthFail, Config, Paths, Scope, TokenStore};
use serde::{Deserialize, Serialize};
use tonic_health::server::HealthReporter;
use tracing::info;

use crate::db::Db;
use crate::health::HealthState;
use crate::model::{Device, Room, Sensor};
use crate::Error;

#[derive(Clone)]
struct ApiState {
    health: HealthState,
    paths: Paths,
    config: Config,
    db: Db,
}

pub async fn serve(config: Config, paths: Paths, health: HealthState, db: Db) -> Result<(), Error> {
    let addr = config.api_addr()?;
    let tls = RustlsConfig::from_pem_file(paths.tls_cert(), paths.tls_key())
        .await
        .map_err(|e| Error::Other(format!("api tls: {e}")))?;

    let app = Router::new()
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
        .route(
            "/api/v1/sensors",
            get(sensors_handler).post(create_sensor_handler),
        )
        .with_state(ApiState {
            health,
            paths,
            config,
            db,
        });

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
    headers: HeaderMap,
) -> Result<Json<StatusBody>, (StatusCode, String)> {
    let rec = authorize(&state.paths, &headers, Scope::Read)?;
    Ok(Json(StatusBody {
        status: "ok",
        llm: state.config.llm.url.clone(),
        stt: state.config.stt.url.clone(),
        tts: state.config.tts.url.clone(),
        wake: state.config.wake.keyword.clone(),
        token_id: rec.id,
    }))
}

async fn house_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<crate::model::House>, (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Read)?;
    let house = state.db.load_house().map_err(db_err)?;
    Ok(Json(house))
}

async fn rooms_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Room>>, (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Read)?;
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<RoomDetail>, (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Read)?;
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
    headers: HeaderMap,
    Json(body): Json<NewRoom>,
) -> Result<(StatusCode, Json<Room>), (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Control)?;
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Control)?;
    if state.db.delete_room(&id).map_err(db_err)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "room not found".into()))
    }
}

async fn devices_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Device>>, (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Read)?;
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
    headers: HeaderMap,
    Json(body): Json<NewDevice>,
) -> Result<(StatusCode, Json<Device>), (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Control)?;
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Control)?;
    if state.db.delete_device(&id).map_err(db_err)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "device not found".into()))
    }
}

async fn sensors_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Sensor>>, (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Read)?;
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
    headers: HeaderMap,
    Json(body): Json<NewSensor>,
) -> Result<(StatusCode, Json<Sensor>), (StatusCode, String)> {
    authorize(&state.paths, &headers, Scope::Control)?;
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

fn authorize(
    paths: &Paths,
    headers: &HeaderMap,
    required: Scope,
) -> Result<homeai_common::TokenRecord, (StatusCode, String)> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "missing authorization".into()))?;
    let secret = raw
        .strip_prefix("Bearer ")
        .ok_or((StatusCode::UNAUTHORIZED, "expected bearer token".into()))?;
    let store = TokenStore::load(paths.tokens_dir()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "token store unreadable".into(),
        )
    })?;
    match store.authorize(secret, required) {
        Ok(rec) => Ok(rec.clone()),
        Err(AuthFail::Unauthorized) => Err((StatusCode::UNAUTHORIZED, "invalid token".into())),
        Err(AuthFail::Forbidden) => Err((StatusCode::FORBIDDEN, "insufficient scope".into())),
    }
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
