//! C2PA content credentials: sign + verify of provenance manifests.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// Watermark metadata carried inside the C2PA manifest assertion
/// (`com.sigil.watermark`), mirroring the pixel-watermark embed parameters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WatermarkClaim {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient_id: Option<String>,
    pub keyed: bool,
}

/// Generate a self-signed ES256 certificate + private key (PEM), c2patool-style.
///
/// Returns `(cert_path, key_path)`. Refuses to overwrite existing files
/// unless `force` is true. The private key is written with 0600 permissions.
pub fn init_cert(org: Option<&str>, out_dir: &Path, force: bool) -> Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("Failed to create dir: {out_dir:?}"))?;
    let cert_path = out_dir.join("cert.pem");
    let key_path = out_dir.join("private.key");

    if !force && (cert_path.exists() || key_path.exists()) {
        return Err(anyhow!(
            "cert.pem/private.key already exist in {out_dir:?}; use --force to overwrite"
        ));
    }

    let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let mut params = rcgen::CertificateParams::new(vec![])?;
    params.distinguished_name.push(
        rcgen::DnType::CommonName,
        org.unwrap_or("Sigil User").to_string(),
    );
    params.not_before = time::OffsetDateTime::now_utc() - time::Duration::hours(1);
    params.not_after = params.not_before + time::Duration::days(730); // 2 years
    let cert = params.self_signed(&key_pair)?;

    std::fs::write(&cert_path, cert.pem())
        .with_context(|| format!("Failed to write {cert_path:?}"))?;
    let key_pem = key_pair.serialize_pem();

    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(&key_path)
        .with_context(|| format!("Failed to write {key_path:?}"))?;
    f.write_all(key_pem.as_bytes())?;

    Ok((cert_path, key_path))
}
