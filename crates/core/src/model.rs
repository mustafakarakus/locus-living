//! Rows stored in `home.db`. House CRUD API is the next slice.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub id: String,
    pub property_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub id: String,
    pub floor_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub room_id: String,
    pub name: String,
    pub kind: String,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceState {
    pub device_id: String,
    pub state_json: String,
    pub updated_ms: i64,
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
