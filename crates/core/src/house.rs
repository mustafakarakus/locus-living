//! Seed `property → floor → room → device/sensor` from `/etc/homeai/house.toml`.

use std::path::Path;

use serde::Deserialize;

use crate::db::{Db, DbError};
use crate::model::{Device, Floor, Property, Room, Sensor};

#[derive(Debug, thiserror::Error)]
pub enum HouseError {
    #[error("cannot read house file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid house file {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error(transparent)]
    Db(#[from] DbError),
}

#[derive(Debug, Clone, Deserialize)]
pub struct HouseFile {
    pub property: PropertyFile,
    #[serde(default)]
    pub floor: Vec<FloorFile>,
    #[serde(default)]
    pub room: Vec<RoomFile>,
    #[serde(default)]
    pub device: Vec<DeviceFile>,
    #[serde(default)]
    pub sensor: Vec<SensorFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyFile {
    #[serde(default = "default_property_id")]
    pub id: String,
    pub name: String,
}

fn default_property_id() -> String {
    "home".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct FloorFile {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoomFile {
    pub id: String,
    pub floor_id: String,
    pub name: String,
    #[serde(default = "default_indoor")]
    pub kind: String,
}

fn default_indoor() -> String {
    "indoor".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceFile {
    pub id: String,
    pub room_id: String,
    pub name: String,
    #[serde(default = "default_unknown")]
    pub kind: String,
    pub protocol: Option<String>,
}

fn default_unknown() -> String {
    "unknown".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SensorFile {
    pub id: String,
    pub room_id: Option<String>,
    pub device_id: Option<String>,
    pub kind: String,
}

impl HouseFile {
    pub fn load(path: &Path) -> Result<Self, HouseError> {
        let raw = std::fs::read_to_string(path).map_err(|source| HouseError::Io {
            path: path.display().to_string(),
            source,
        })?;
        toml::from_str(&raw).map_err(|source| HouseError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn into_rows(self) -> (Property, Vec<Floor>, Vec<Room>, Vec<Device>, Vec<Sensor>) {
        let property = Property {
            id: self.property.id.clone(),
            name: self.property.name,
        };
        let floors = self
            .floor
            .into_iter()
            .map(|f| Floor {
                id: f.id,
                property_id: property.id.clone(),
                name: f.name,
            })
            .collect();
        let rooms = self
            .room
            .into_iter()
            .map(|r| Room {
                id: r.id,
                floor_id: r.floor_id,
                name: r.name,
                kind: r.kind,
            })
            .collect();
        let devices = self
            .device
            .into_iter()
            .map(|d| Device {
                id: d.id,
                room_id: d.room_id,
                name: d.name,
                kind: d.kind,
                protocol: d.protocol,
            })
            .collect();
        let sensors = self
            .sensor
            .into_iter()
            .map(|s| Sensor {
                id: s.id,
                room_id: s.room_id,
                device_id: s.device_id,
                kind: s.kind,
            })
            .collect();
        (property, floors, rooms, devices, sensors)
    }
}

/// Load `house.toml` once if the database has no property yet. Missing file is a no-op.
pub fn seed_if_empty(db: &Db, path: &Path) -> Result<bool, HouseError> {
    if !path.is_file() {
        tracing::info!("no house.toml; starting with an empty house");
        return Ok(false);
    }
    if db.count("property")? > 0 {
        return Ok(false);
    }
    let spec = HouseFile::load(path)?;
    let (property, floors, rooms, devices, sensors) = spec.into_rows();
    db.put_property(property)?;
    for row in floors {
        db.put_floor(row)?;
    }
    for row in rooms {
        db.put_room(row)?;
    }
    for row in devices {
        db.put_device(row)?;
    }
    for row in sensors {
        db.put_sensor(row)?;
    }
    Ok(true)
}
