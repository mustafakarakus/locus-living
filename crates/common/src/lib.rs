//! Shared constants and paths. Matches `docs/techstack.md` §3–§4.

pub const API_PORT: u16 = 8443;
pub const GRPC_PORT: u16 = 50051;
pub const LLM_PORT: u16 = 8200;
pub const VISION_PORT: u16 = 8250;
pub const STT_PORT: u16 = 8100;
pub const TTS_PORT: u16 = 8300;
pub const METRICS_PORT: u16 = 8500;
pub const GRAFANA_PORT: u16 = 3000;
pub const MQTTS_PORT: u16 = 8883;

pub const CONFIG_PATH: &str = "/etc/homeai/config.toml";
pub const HOUSE_PATH: &str = "/etc/homeai/house.toml";
pub const MODELS_PATH: &str = "/etc/homeai/models.toml";
pub const DB_PATH: &str = "/var/lib/homeai/home.db";
