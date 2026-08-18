//! Rows stored in `home.db`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Property {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Floor {
    pub id: String,
    pub property_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub floor_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub room_id: String,
    pub name: String,
    pub kind: String,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    pub device_id: String,
    pub state_json: String,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sensor {
    pub id: String,
    pub room_id: Option<String>,
    pub device_id: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct House {
    pub property: Option<Property>,
    pub floors: Vec<Floor>,
    pub rooms: Vec<Room>,
    pub devices: Vec<Device>,
    pub sensors: Vec<Sensor>,
}

/// Tables required by the local store (techstack §9).
pub const REQUIRED_TABLES: &[&str] = &[
    "property",
    "floor",
    "room",
    "device",
    "sensor",
    "device_state",
    "person",
    "identity_signal",
    "presence_event",
    "automation",
    "automation_rule",
    "automation_execution",
    "event_log",
    "home_memory",
    "user_preference",
    "conversation_session",
    "conversation_attachment",
    "system_health",
];
