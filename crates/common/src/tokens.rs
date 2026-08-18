//! Bearer tokens under `$tls/tokens/*.toml`, mode 0600 (UC-103).

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("token io: {0}")]
    Io(#[from] io::Error),
    #[error("token file {path} is world-readable (must be 0600)")]
    WorldReadable { path: String },
    #[error("invalid token file {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("unknown token id {0}")]
    NotFound(String),
    #[error("token id {0} already exists")]
    Exists(String),
    #[error("invalid token id {0}")]
    BadId(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Read,
    Control,
    Admin,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Control => "control",
            Scope::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRecord {
    pub id: String,
    pub secret: String,
    pub scopes: Vec<Scope>,
    pub created_ms: i64,
}

impl TokenRecord {
    pub fn allows(&self, required: Scope) -> bool {
        self.scopes.contains(&Scope::Admin) || self.scopes.contains(&required)
    }
}

#[derive(Clone, Debug, Default)]
pub struct TokenStore {
    dir: PathBuf,
    /// secret → record
    by_secret: BTreeMap<String, TokenRecord>,
    by_id: BTreeMap<String, TokenRecord>,
}

impl TokenStore {
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self, TokenError> {
        let dir = dir.into();
        let mut store = Self {
            dir: dir.clone(),
            by_secret: BTreeMap::new(),
            by_id: BTreeMap::new(),
        };
        if !dir.exists() {
            return Ok(store);
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            reject_world_readable(&path)?;
            let raw = fs::read_to_string(&path)?;
            let rec: TokenRecord = toml::from_str(&raw).map_err(|source| TokenError::Parse {
                path: path.display().to_string(),
                source,
            })?;
            store.by_secret.insert(rec.secret.clone(), rec.clone());
            store.by_id.insert(rec.id.clone(), rec);
        }
        Ok(store)
    }

    pub fn get(&self, secret: &str) -> Option<&TokenRecord> {
        self.by_secret.get(secret)
    }

    pub fn authorize(&self, secret: &str, required: Scope) -> Result<&TokenRecord, AuthFail> {
        match self.by_secret.get(secret) {
            None => Err(AuthFail::Unauthorized),
            Some(rec) if rec.allows(required) => Ok(rec),
            Some(_) => Err(AuthFail::Forbidden),
        }
    }

    pub fn list(&self) -> Vec<&TokenRecord> {
        self.by_id.values().collect()
    }

    pub fn create(&mut self, id: &str, scopes: Vec<Scope>) -> Result<TokenRecord, TokenError> {
        validate_id(id)?;
        if self.by_id.contains_key(id) {
            return Err(TokenError::Exists(id.into()));
        }
        let rec = new_record(id, scopes);
        write_record(&self.dir, &rec)?;
        self.by_secret.insert(rec.secret.clone(), rec.clone());
        self.by_id.insert(rec.id.clone(), rec.clone());
        Ok(rec)
    }

    pub fn revoke(&mut self, id: &str) -> Result<(), TokenError> {
        let rec = self
            .by_id
            .remove(id)
            .ok_or_else(|| TokenError::NotFound(id.into()))?;
        self.by_secret.remove(&rec.secret);
        let path = self.dir.join(format!("{id}.toml"));
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn rotate(&mut self, id: &str) -> Result<TokenRecord, TokenError> {
        let old = self
            .by_id
            .get(id)
            .ok_or_else(|| TokenError::NotFound(id.into()))?
            .clone();
        self.by_secret.remove(&old.secret);
        let rec = new_record(id, old.scopes);
        write_record(&self.dir, &rec)?;
        self.by_secret.insert(rec.secret.clone(), rec.clone());
        self.by_id.insert(rec.id.clone(), rec.clone());
        Ok(rec)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFail {
    Unauthorized,
    Forbidden,
}

fn validate_id(id: &str) -> Result<(), TokenError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(TokenError::BadId(id.into()));
    }
    Ok(())
}

fn new_record(id: &str, scopes: Vec<Scope>) -> TokenRecord {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    TokenRecord {
        id: id.into(),
        secret: hex(&bytes),
        scopes,
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_record(dir: &Path, rec: &TokenRecord) -> Result<(), TokenError> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dir)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(dir, perms)?;
    }
    let path = dir.join(format!("{}.toml", rec.id));
    let body = toml::to_string_pretty(rec).expect("token toml");
    write_mode_600(&path, body.as_bytes())?;
    Ok(())
}

fn write_mode_600(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
    }
}

fn reject_world_readable(path: &Path) -> Result<(), TokenError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o044 != 0 {
            return Err(TokenError::WorldReadable {
                path: path.display().to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_is_0600_scoped_and_revocable() {
        let dir = std::env::temp_dir().join(format!("homeai-tok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut store = TokenStore::load(&dir).unwrap();
        let rec = store
            .create("phone", vec![Scope::Read, Scope::Control])
            .unwrap();
        let path = dir.join("phone.toml");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let loaded = TokenStore::load(&dir).unwrap();
        assert!(loaded.authorize(&rec.secret, Scope::Read).is_ok());
        assert!(loaded.authorize(&rec.secret, Scope::Control).is_ok());
        assert_eq!(
            loaded.authorize(&rec.secret, Scope::Admin),
            Err(AuthFail::Forbidden)
        );
        assert_eq!(
            loaded.authorize("nope", Scope::Read),
            Err(AuthFail::Unauthorized)
        );

        let mut store = TokenStore::load(&dir).unwrap();
        store.revoke("phone").unwrap();
        let loaded = TokenStore::load(&dir).unwrap();
        assert!(loaded.get(&rec.secret).is_none());
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn admin_scope_satisfies_read() {
        let rec = TokenRecord {
            id: "a".into(),
            secret: "s".into(),
            scopes: vec![Scope::Admin],
            created_ms: 1,
        };
        assert!(rec.allows(Scope::Read));
        assert!(rec.allows(Scope::Control));
        assert!(rec.allows(Scope::Admin));
    }

    #[test]
    fn world_readable_token_is_rejected() {
        let dir = std::env::temp_dir().join(format!("homeai-tok-open-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("open.toml");
        fs::write(
            &path,
            "id = \"open\"\nsecret = \"abc\"\nscopes = [\"read\"]\ncreated_ms = 1\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&path).unwrap().permissions();
            p.set_mode(0o644);
            fs::set_permissions(&path, p).unwrap();
            let err = TokenStore::load(&dir).unwrap_err();
            assert!(matches!(err, TokenError::WorldReadable { .. }));
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
