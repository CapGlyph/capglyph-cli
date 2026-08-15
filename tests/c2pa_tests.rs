//! C2PA integration tests — require the `c2pa` cargo feature.
#![cfg(feature = "c2pa")]

use sigil::c2pa::{init_cert, sign_image, WatermarkClaim};

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

fn make_fixture_rgb(w: u32, h: u32) -> image::RgbImage {
    image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([
            (x * 7 % 256) as u8,
            (y * 11 % 256) as u8,
            ((x + y) * 3 % 256) as u8,
        ])
    })
}

fn sign_roundtrip(ext: &str, source_type: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = init_cert(Some("Sigil Test"), dir.path(), false).unwrap();

    let input = dir.path().join(format!("input.{ext}"));
    let output = dir.path().join(format!("signed.{ext}"));
    let img = make_fixture_rgb(64, 64);
    img.save(&input).unwrap();

    let claim = WatermarkClaim {
        mode: "dct".to_string(),
        recipient_id: Some("alice01".to_string()),
        keyed: false,
    };
    sign_image(&input, &output, &cert, &key, &claim, None, source_type).unwrap();
    (dir, output)
}

#[test]
fn sign_image_png_produces_valid_manifest() {
    let (_dir, output) = sign_roundtrip("png", None);
    let reader = c2pa::Reader::from_context(c2pa::Context::new())
        .with_file(&output)
        .unwrap();
    let manifest = reader.active_manifest().expect("manifest present");
    assert_eq!(reader.validation_state(), c2pa::ValidationState::Valid);

    let results = reader.validation_results().expect("validation results");
    let statuses = results.active_manifest().expect("active manifest statuses");
    let success: Vec<&str> = statuses.success.iter().map(|s| s.code()).collect();
    assert!(
        success.contains(&"claimSignature.validated"),
        "success codes: {success:?}"
    );
    let failures: Vec<&str> = statuses.failure.iter().map(|s| s.code()).collect();
    assert!(
        !failures.contains(&"claimSignature.mismatch"),
        "failure codes: {failures:?}"
    );

    let claim: WatermarkClaim = manifest.find_assertion("com.sigil.watermark").unwrap();
    assert_eq!(claim.mode, "dct");
    assert_eq!(claim.recipient_id.as_deref(), Some("alice01"));
}

#[test]
fn sign_image_jpeg_produces_valid_manifest() {
    let (_dir, output) = sign_roundtrip("jpg", None);
    let reader = c2pa::Reader::from_context(c2pa::Context::new())
        .with_file(&output)
        .unwrap();
    assert!(reader.active_manifest().is_some());
    assert_eq!(reader.validation_state(), c2pa::ValidationState::Valid);

    let results = reader.validation_results().expect("validation results");
    let statuses = results.active_manifest().expect("active manifest statuses");
    let success: Vec<&str> = statuses.success.iter().map(|s| s.code()).collect();
    assert!(
        success.contains(&"claimSignature.validated"),
        "success codes: {success:?}"
    );
}

#[test]
fn sign_image_rejects_in_place_signing() {
    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = init_cert(None, dir.path(), false).unwrap();
    let input = dir.path().join("input.png");
    make_fixture_rgb(32, 32).save(&input).unwrap();
    let claim = WatermarkClaim {
        mode: "dct".to_string(),
        recipient_id: None,
        keyed: false,
    };
    let err = sign_image(&input, &input, &cert, &key, &claim, None, None).unwrap_err();
    assert!(err.to_string().contains("same path"));
}

#[test]
fn sign_image_wrong_key_fails_validation() {
    // cert from pair A, key from pair B -> validation must fail
    let dir = tempfile::tempdir().unwrap();
    let (cert_a, _key_a) = init_cert(None, &dir.path().join("a"), false).unwrap();
    let (_cert_b, key_b) = init_cert(None, &dir.path().join("b"), false).unwrap();

    let input = dir.path().join("input.png");
    let output = dir.path().join("signed.png");
    make_fixture_rgb(32, 32).save(&input).unwrap();
    let claim = WatermarkClaim {
        mode: "dct".to_string(),
        recipient_id: None,
        keyed: false,
    };
    sign_image(&input, &output, &cert_a, &key_b, &claim, None, None).unwrap();

    let reader = c2pa::Reader::from_context(c2pa::Context::new())
        .with_file(&output)
        .unwrap();
    assert_ne!(reader.validation_state(), c2pa::ValidationState::Valid);
    let results = reader.validation_results().expect("validation results");
    let statuses = results.active_manifest().expect("active manifest statuses");
    let success: Vec<&str> = statuses.success.iter().map(|s| s.code()).collect();
    let failures: Vec<&str> = statuses.failure.iter().map(|s| s.code()).collect();
    assert!(
        !success.contains(&"claimSignature.validated"),
        "mismatched key must not validate, success codes: {success:?}"
    );
    assert!(
        failures.contains(&"claimSignature.mismatch"),
        "mismatched key must fail signature verification, failure codes: {failures:?}"
    );
}

fn signed_actions(output: &std::path::Path) -> c2pa::assertions::Actions {
    let reader = c2pa::Reader::from_context(c2pa::Context::new())
        .with_file(output)
        .unwrap();
    assert_eq!(reader.validation_state(), c2pa::ValidationState::Valid);
    let manifest = reader.active_manifest().expect("manifest present");
    manifest.find_assertion("c2pa.actions.v2").unwrap()
}

#[test]
fn sign_image_defaults_to_digital_capture_no_false_ai_attestation() {
    let (_dir, output) = sign_roundtrip("png", None);
    let actions = signed_actions(&output);
    assert_eq!(actions.actions().len(), 1);
    let created = &actions.actions()[0];
    assert_eq!(created.action(), "c2pa.created");
    assert_eq!(
        created.source_type(),
        Some(&c2pa::assertions::DigitalSourceType::DigitalCapture),
        "default source type must attest capture, never AI origin"
    );
}

#[test]
fn sign_image_honors_short_source_type_token() {
    let (_dir, output) = sign_roundtrip("png", Some("algorithmic"));
    let actions = signed_actions(&output);
    assert_eq!(
        actions.actions()[0].source_type(),
        Some(&c2pa::assertions::DigitalSourceType::AlgorithmicMedia)
    );
}

#[test]
fn sign_image_accepts_full_source_type_uri() {
    let (_dir, output) = sign_roundtrip(
        "png",
        Some("http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"),
    );
    let actions = signed_actions(&output);
    assert_eq!(
        actions.actions()[0].source_type(),
        Some(&c2pa::assertions::DigitalSourceType::TrainedAlgorithmicMedia)
    );
}

#[test]
fn sign_image_rejects_unknown_source_type() {
    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = init_cert(None, dir.path(), false).unwrap();
    let input = dir.path().join("input.png");
    let output = dir.path().join("signed.png");
    make_fixture_rgb(32, 32).save(&input).unwrap();
    let claim = WatermarkClaim {
        mode: "dct".to_string(),
        recipient_id: None,
        keyed: false,
    };
    let err = sign_image(&input, &output, &cert, &key, &claim, None, Some("bogus")).unwrap_err();
    assert!(err.to_string().contains("unknown digital source type"));
}
