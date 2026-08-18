use std::fs;
use std::path::Path;

use homeai_common::Paths;

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("missing TLS material {0} (provisioning writes /etc/homeai/tls/)")]
    Missing(String),
    #[error("tls io: {0}")]
    Io(#[from] std::io::Error),
    #[error("rcgen: {0}")]
    Rcgen(#[from] rcgen::Error),
}

pub fn require_server_certs(paths: &Paths) -> Result<(), TlsError> {
    let cert = paths.tls_cert();
    let key = paths.tls_key();
    if !cert.is_file() {
        return Err(TlsError::Missing(cert.display().to_string()));
    }
    if !key.is_file() {
        return Err(TlsError::Missing(key.display().to_string()));
    }
    Ok(())
}

/// Self-signed material for `HOMEAI_PREFIX` only. Production must be provisioned.
pub fn ensure_dev_certs(paths: &Paths) -> Result<(), TlsError> {
    let cert_path = paths.tls_cert();
    let key_path = paths.tls_key();
    if cert_path.is_file() && key_path.is_file() {
        return Ok(());
    }
    write_self_signed(&cert_path, &key_path)
}

pub fn write_self_signed(cert_path: &Path, key_path: &Path) -> Result<(), TlsError> {
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut params = rcgen::CertificateParams::new(vec![
        "localhost".into(),
        "127.0.0.1".into(),
        "homeai.local".into(),
    ])?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "homeai-dev");
    let key = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    fs::write(cert_path, cert.pem())?;
    fs::write(key_path, key.serialize_pem())?;
    Ok(())
}
