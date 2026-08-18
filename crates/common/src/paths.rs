use std::env;
use std::path::{Path, PathBuf};

/// Resolved filesystem layout. Production uses the techstack §4 absolute paths.
/// When `HOMEAI_PREFIX` is set, the same tree is rooted under that directory.
#[derive(Debug, Clone)]
pub struct Paths {
    prefix: Option<PathBuf>,
    pub config: PathBuf,
    pub house: PathBuf,
    pub models: PathBuf,
    pub tls_dir: PathBuf,
    pub db: PathBuf,
    pub log_dir: PathBuf,
    pub updates: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        match env::var_os("HOMEAI_PREFIX") {
            Some(p) if !p.is_empty() => Self::prefixed(p),
            _ => Self::production(),
        }
    }

    pub fn production() -> Self {
        Self {
            prefix: None,
            config: PathBuf::from("/etc/homeai/config.toml"),
            house: PathBuf::from("/etc/homeai/house.toml"),
            models: PathBuf::from("/etc/homeai/models.toml"),
            tls_dir: PathBuf::from("/etc/homeai/tls"),
            db: PathBuf::from("/var/lib/homeai/home.db"),
            log_dir: PathBuf::from("/var/log/homeai"),
            updates: PathBuf::from("/var/lib/homeai/updates"),
        }
    }

    pub fn prefixed(prefix: impl Into<PathBuf>) -> Self {
        let prefix = prefix.into();
        Self {
            config: prefix.join("etc/homeai/config.toml"),
            house: prefix.join("etc/homeai/house.toml"),
            models: prefix.join("etc/homeai/models.toml"),
            tls_dir: prefix.join("etc/homeai/tls"),
            db: prefix.join("var/lib/homeai/home.db"),
            log_dir: prefix.join("var/log/homeai"),
            updates: prefix.join("var/lib/homeai/updates"),
            prefix: Some(prefix),
        }
    }

    pub fn is_prefixed(&self) -> bool {
        self.prefix.is_some()
    }

    pub fn prefix(&self) -> Option<&Path> {
        self.prefix.as_deref()
    }

    pub fn core_log(&self) -> PathBuf {
        self.log_dir.join("core.log")
    }

    pub fn tls_cert(&self) -> PathBuf {
        self.tls_dir.join("cert.pem")
    }

    pub fn tls_key(&self) -> PathBuf {
        self.tls_dir.join("key.pem")
    }

    pub fn ensure_runtime_dirs(&self) -> std::io::Result<()> {
        if let Some(parent) = self.db.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&self.log_dir)?;
        std::fs::create_dir_all(&self.updates)?;
        std::fs::create_dir_all(&self.tls_dir)?;
        if let Some(parent) = self.config.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_paths_match_techstack() {
        let p = Paths::production();
        assert_eq!(p.config, PathBuf::from("/etc/homeai/config.toml"));
        assert_eq!(p.db, PathBuf::from("/var/lib/homeai/home.db"));
        assert_eq!(p.core_log(), PathBuf::from("/var/log/homeai/core.log"));
        assert!(!p.is_prefixed());
    }

    #[test]
    fn prefix_mirrors_the_same_tree() {
        let p = Paths::prefixed("/tmp/homeai-dev");
        assert_eq!(
            p.config,
            PathBuf::from("/tmp/homeai-dev/etc/homeai/config.toml")
        );
        assert_eq!(
            p.db,
            PathBuf::from("/tmp/homeai-dev/var/lib/homeai/home.db")
        );
        assert!(p.is_prefixed());
    }
}
