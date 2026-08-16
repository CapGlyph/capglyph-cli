//! C2PA integration tests — require the `c2pa` cargo feature.
#![cfg(feature = "c2pa")]

use sigil::c2pa::{init_cert, sign_image, verify_image, WatermarkClaim};

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

#[test]
fn verify_image_unsigned() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("plain.png");
    make_fixture_rgb(32, 32).save(&input).unwrap();
    let report = verify_image(&input).unwrap();
    assert!(!report.present);
    assert_eq!(report.signature_status, "unsigned");
    assert!(report.signer_org.is_none());
    assert!(report.watermark_claim.is_none());
}

#[test]
fn verify_image_signed_reports_org_and_claim() {
    let (_dir, output) = sign_roundtrip("png", None);
    let report = verify_image(&output).unwrap();
    assert!(report.present);
    assert_eq!(report.signature_status, "valid");
    assert_eq!(report.signer_org.as_deref(), Some("Sigil Test"));
    assert!(report.valid_from.is_some());
    assert!(report.valid_to.is_some());
    let claim = report.watermark_claim.expect("claim present");
    assert_eq!(claim.mode, "dct");
}

#[test]
fn cli_parses_c2pa_init_sign_verify() {
    use clap::Parser;
    use sigil::cli::{Cli, Commands};

    let cli =
        Cli::try_parse_from(["sigil", "c2pa", "init", "--org", "Acme", "--out", "/tmp/x"]).unwrap();
    match &cli.command {
        Commands::C2pa(c) => match &c.command {
            sigil::cli::C2paCommand::Init(i) => {
                assert_eq!(i.org.as_deref(), Some("Acme"));
                assert_eq!(i.out.as_deref().unwrap().to_str(), Some("/tmp/x"));
            }
            _ => panic!("wrong subcommand"),
        },
        _ => panic!("wrong command"),
    }

    let cli = Cli::try_parse_from([
        "sigil", "c2pa", "sign", "in.png", "--cert", "c.pem", "--pkey", "k.key", "-o", "out.png",
    ])
    .unwrap();
    match &cli.command {
        Commands::C2pa(c) => match &c.command {
            sigil::cli::C2paCommand::Sign(s) => {
                assert_eq!(s.input.to_str(), Some("in.png"));
                assert!(s.recipient_id.is_none());
                assert_eq!(s.source_type, "capture");
            }
            _ => panic!("wrong subcommand"),
        },
        _ => panic!("wrong command"),
    }

    let cli = Cli::try_parse_from(["sigil", "c2pa", "verify", "in.png"]).unwrap();
    match &cli.command {
        Commands::C2pa(c) => match &c.command {
            sigil::cli::C2paCommand::Verify(v) => assert_eq!(v.input.to_str(), Some("in.png")),
            _ => panic!("wrong subcommand"),
        },
        _ => panic!("wrong command"),
    }
}

#[test]
fn cli_c2pa_sign_recipient_id_requires_mode() {
    use clap::Parser;
    use sigil::cli::Cli;
    assert!(Cli::try_parse_from([
        "sigil",
        "c2pa",
        "sign",
        "in.png",
        "--cert",
        "c.pem",
        "--pkey",
        "k.key",
        "--recipient-id",
        "bob",
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "sigil",
        "c2pa",
        "sign",
        "in.png",
        "--cert",
        "c.pem",
        "--pkey",
        "k.key",
        "--recipient-id",
        "bob",
        "--mode",
        "dct",
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "sigil", "c2pa", "sign", "in.png", "--cert", "c.pem", "--pkey", "k.key", "--mode", "dct",
    ])
    .is_err());
}

#[test]
fn embed_with_c2pa_signs_output_consistently() {
    use sigil::cli::EmbedArgs;
    use sigil::embed;

    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = sigil::c2pa::init_cert(Some("Dual Layer"), dir.path(), false).unwrap();

    let input = dir.path().join("input.png");
    let output = dir.path().join("output.png");
    // 512×512 checkerboard-style gradient: recipient-id DCT embedding needs
    // enough 8×8 blocks (128×128 has only 256 blocks < 512 needed → hang).
    make_fixture_rgb(512, 512).save(&input).unwrap();

    let args = EmbedArgs {
        input: input.clone(),
        output: Some(output.clone()),
        mode: sigil::cli::EmbedMode::Dct,
        stroke: 0.010,
        detail: 60,
        min_path_len: 5,
        chaikin_iters: 3,
        color: false,
        key: None,
        save_geometry: None,
        from_geometry: None,
        recipient_id: Some("carol07".to_string()),
        model_dir: None,
        strength: 0.95,
        c2pa: true,
        c2pa_cert: Some(cert),
        c2pa_pkey: Some(key),
    };
    embed::run(&args).unwrap();

    let report = sigil::c2pa::verify_image(&output).unwrap();
    assert!(report.present);
    assert_eq!(report.signature_status, "valid");
    let claim = report.watermark_claim.expect("watermark claim");
    assert_eq!(claim.mode, "dct");
    assert_eq!(claim.recipient_id.as_deref(), Some("carol07"));
    assert!(!claim.keyed);
}

#[test]
fn verify_with_c2pa_reports_manifest() {
    use sigil::cli::VerifyArgs;
    use sigil::verify;

    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = sigil::c2pa::init_cert(None, dir.path(), false).unwrap();
    let input = dir.path().join("input.png");
    let output = dir.path().join("output.png");
    make_fixture_rgb(512, 512).save(&input).unwrap();

    // sign via the c2pa module directly (pixel watermark not needed for the
    // report linkage test)
    let claim = sigil::c2pa::WatermarkClaim {
        mode: "dct".to_string(),
        recipient_id: Some("dave01".to_string()),
        keyed: false,
    };
    sigil::c2pa::sign_image(&input, &output, &cert, &key, &claim, None, None).unwrap();

    let args = VerifyArgs {
        input: output.clone(),
        mode: sigil::cli::EmbedMode::Dct,
        geometry: None,
        threshold: 0.0001,
        mean_threshold: 4.0,
        key: None,
        model_dir: None,
        recipient_id: None,
        verbose: false,
        c2pa: true,
    };
    let present = verify::run(&args).unwrap();
    // pixel watermark absent (only the manifest was signed), but C2PA section
    // must have been produced without errors
    assert!(!present);
}
