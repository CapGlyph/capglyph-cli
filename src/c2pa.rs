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
    params
        .distinguished_name
        .push(rcgen::DnType::OrganizationName, "Sigil");
    params.not_before = time::OffsetDateTime::now_utc() - time::Duration::hours(1);
    params.not_after = params.not_before + time::Duration::days(730); // 2 years
    params.use_authority_key_identifier_extension = true;
    params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::EmailProtection];
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

/// Sign `input` with a C2PA manifest carrying `claim` as the
/// `com.sigil.watermark` assertion, writing to `output`.
///
/// `manifest_json` optionally merges extra assertions:
/// `{"label": <json value>, ...}`.
pub fn sign_image(
    input: &Path,
    output: &Path,
    cert: &Path,
    pkey: &Path,
    claim: &WatermarkClaim,
    manifest_json: Option<&Path>,
) -> Result<()> {
    if input == output {
        return Err(anyhow!(
            "in-place signing is not supported; input and output must not be the same path \
             (specify -o <out>)"
        ));
    }

    let signer = c2pa::create_signer::from_files(cert, pkey, c2pa::SigningAlg::Es256, None)
        .with_context(|| format!("Failed to load signing key pair from {pkey:?}"))?;

    let mut builder = c2pa::Builder::from_context(c2pa::Context::new())
        .with_definition(r#"{"claim_generator": "sigil"}"#)
        .context("Failed to create manifest builder")?;
    let mut claim_generator_info = c2pa::ClaimGeneratorInfo::new("sigil");
    claim_generator_info.set_version(env!("CARGO_PKG_VERSION"));
    builder.set_claim_generator_info(claim_generator_info);

    let actions = c2pa::assertions::Actions::new().add_action(
        c2pa::assertions::Action::new(c2pa::assertions::c2pa_action::CREATED)
            .set_source_type(c2pa::assertions::DigitalSourceType::AlgorithmicMedia),
    );
    builder
        .add_assertion(c2pa::assertions::Actions::LABEL_VERSIONED, &actions)
        .context("Failed to add actions assertion")?;

    builder
        .add_assertion("com.sigil.watermark", claim)
        .context("Failed to add watermark assertion")?;

    if let Some(path) = manifest_json {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read manifest JSON: {path:?}"))?;
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&raw).context("manifest JSON must be an object")?;
        for (label, value) in map {
            builder
                .add_assertion_json(label, &value)
                .context("Failed to add extra assertion")?;
        }
    }

    builder
        .sign_file(signer.as_ref(), input, output)
        .with_context(|| format!("Failed to sign {input:?} -> {output:?}"))?;
    Ok(())
}
