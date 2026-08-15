//! C2PA integration tests — require the `c2pa` cargo feature.
#![cfg(feature = "c2pa")]

use sigil::c2pa::{init_cert, WatermarkClaim};

#[test]
fn watermark_claim_serde_roundtrip() {
    let claim = WatermarkClaim {
        mode: "dct".to_string(),
        recipient_id: Some("alice01".to_string()),
        keyed: true,
    };
    let json = serde_json::to_vec(&claim).unwrap();
    // Some recipient_id must be present in the JSON
    let raw = std::str::from_utf8(&json).unwrap();
    assert!(raw.contains("recipient_id"));
    let back: WatermarkClaim = serde_json::from_slice(&json).unwrap();
    assert_eq!(back.mode, "dct");
    assert_eq!(back.recipient_id.as_deref(), Some("alice01"));
    assert!(back.keyed);
}

#[test]
fn watermark_claim_none_fields() {
    let claim = WatermarkClaim {
        mode: "dwt".to_string(),
        recipient_id: None,
        keyed: false,
    };
    let json = serde_json::to_vec(&claim).unwrap();
    // skip_serializing_if: None recipient_id must be omitted from the JSON
    let raw = std::str::from_utf8(&json).unwrap();
    assert!(!raw.contains("recipient_id"));
    let back: WatermarkClaim = serde_json::from_slice(&json).unwrap();
    assert_eq!(back.mode, "dwt");
    assert_eq!(back.recipient_id, None);
    assert!(!back.keyed);
}

#[test]
fn init_cert_generates_valid_pair() {
    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = init_cert(Some("Test Org"), dir.path(), false).unwrap();
    assert_eq!(cert.file_name().unwrap(), "cert.pem");
    assert_eq!(key.file_name().unwrap(), "private.key");

    let cert_pem = std::fs::read(&cert).unwrap();
    let key_pem = std::fs::read_to_string(&key).unwrap();

    // cert parses and carries the requested CN
    let (_, pem) = x509_parser::pem::parse_x509_pem(&cert_pem).unwrap();
    let x509 = pem.parse_x509().unwrap();
    assert!(x509.subject().to_string().contains("Test Org"));

    // key is PKCS-8 PEM and loads as a P256 signer in c2pa
    let signer =
        c2pa::create_signer::from_files(&cert, &key, c2pa::SigningAlg::Es256, None).unwrap();
    let _ = signer.alg(); // does not panic

    // private key is 0600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&key).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert!(key_pem.contains("BEGIN PRIVATE KEY"));
}

#[test]
fn init_cert_refuses_overwrite_without_force() {
    let dir = tempfile::tempdir().unwrap();
    init_cert(None, dir.path(), false).unwrap();
    assert!(init_cert(None, dir.path(), false).is_err());
    assert!(init_cert(None, dir.path(), true).is_ok());
}
