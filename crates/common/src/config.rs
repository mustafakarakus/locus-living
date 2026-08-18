use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use serde::Deserialize;

use crate::{API_PORT, GRPC_PORT, LLM_PORT, STT_PORT, TTS_PORT};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid config {path}: {message}")]
    Invalid { path: String, message: String },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub grpc: GrpcConfig,
    #[serde(default)]
    pub llm: ServiceUrl,
    #[serde(default = "default_stt")]
    pub stt: ServiceUrl,
    #[serde(default = "default_tts")]
    pub tts: ServiceUrl,
    #[serde(default)]
    pub presence: PresenceConfig,
    #[serde(default)]
    pub wake: WakeConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ApiConfig {
    #[serde(default = "default_api_host")]
    pub host: String,
    #[serde(default = "default_api_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GrpcConfig {
    #[serde(default = "default_grpc_host")]
    pub host: String,
    #[serde(default = "default_grpc_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServiceUrl {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PresenceConfig {
    #[serde(default = "default_exit_delay")]
    pub exit_delay_ms: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WakeConfig {
    #[serde(default = "default_wake")]
    pub keyword: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: default_api_host(),
            port: default_api_port(),
        }
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            host: default_grpc_host(),
            port: default_grpc_port(),
        }
    }
}

impl Default for ServiceUrl {
    fn default() -> Self {
        Self {
            url: format!("http://127.0.0.1:{LLM_PORT}"),
        }
    }
}

fn default_stt() -> ServiceUrl {
    ServiceUrl {
        url: format!("http://127.0.0.1:{STT_PORT}"),
    }
}

fn default_tts() -> ServiceUrl {
    ServiceUrl {
        url: format!("http://127.0.0.1:{TTS_PORT}"),
    }
}

impl Default for PresenceConfig {
    fn default() -> Self {
        Self {
            exit_delay_ms: default_exit_delay(),
        }
    }
}

impl Default for WakeConfig {
    fn default() -> Self {
        Self {
            keyword: default_wake(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api: ApiConfig::default(),
            grpc: GrpcConfig::default(),
            llm: ServiceUrl::default(),
            stt: default_stt(),
            tts: default_tts(),
            presence: PresenceConfig::default(),
            wake: WakeConfig::default(),
        }
    }
}

fn default_api_host() -> String {
    "0.0.0.0".into()
}

fn default_api_port() -> u16 {
    API_PORT
}

fn default_grpc_host() -> String {
    "0.0.0.0".into()
}

fn default_grpc_port() -> u16 {
    GRPC_PORT
}

fn default_exit_delay() -> u64 {
    30_000
}

fn default_wake() -> String {
    "hey home".into()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let cfg: Self = toml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.display().to_string(),
            source,
        })?;
        cfg.validate(path)?;
        Ok(cfg)
    }

    pub fn validate(&self, path: &Path) -> Result<(), ConfigError> {
        let invalid = |message: String| ConfigError::Invalid {
            path: path.display().to_string(),
            message,
        };
        if self.api.port == 0 {
            return Err(invalid("api.port must be non-zero".into()));
        }
        if self.grpc.port == 0 {
            return Err(invalid("grpc.port must be non-zero".into()));
        }
        check_http_url(&self.llm.url).map_err(|m| invalid(format!("llm.url: {m}")))?;
        check_http_url(&self.stt.url).map_err(|m| invalid(format!("stt.url: {m}")))?;
        check_http_url(&self.tts.url).map_err(|m| invalid(format!("tts.url: {m}")))?;
        if self.presence.exit_delay_ms == 0 {
            return Err(invalid("presence.exit_delay_ms must be > 0".into()));
        }
        if self.wake.keyword.trim().is_empty() {
            return Err(invalid("wake.keyword must not be empty".into()));
        }
        Ok(())
    }

    pub fn api_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.api.host, self.api.port).parse()
    }

    pub fn grpc_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.grpc.host, self.grpc.port).parse()
    }
}

fn check_http_url(s: &str) -> Result<(), String> {
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return Err("must start with http:// or https://".into());
    }
    if s.contains(char::is_whitespace) {
        return Err("must not contain whitespace".into());
    }
    let rest = s
        .split_once("://")
        .map(|(_, r)| r)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| "missing host".to_string())?;
    if rest.is_empty() {
        return Err("missing host".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn defaults_match_techstack() {
        let cfg = Config::default();
        assert_eq!(cfg.api.port, 8443);
        assert_eq!(cfg.grpc.port, 50051);
        assert_eq!(cfg.llm.url, "http://127.0.0.1:8200");
        assert_eq!(cfg.stt.url, "http://127.0.0.1:8100");
        assert_eq!(cfg.tts.url, "http://127.0.0.1:8300");
        assert_eq!(cfg.presence.exit_delay_ms, 30_000);
        assert_eq!(cfg.wake.keyword, "hey home");
    }

    #[test]
    fn malformed_toml_is_a_hard_error() {
        let dir = std::env::temp_dir().join(format!("homeai-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "api = ???").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_a_hard_error() {
        let err = Config::load(Path::new("/no/such/homeai/config.toml")).unwrap_err();
        assert!(matches!(err, ConfigError::Io { .. }));
    }

    #[test]
    fn bad_llm_url_fails_validate() {
        let dir = tempfile_dir();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[llm]\nurl = \"not-a-url\"\n").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "homeai-cfg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
