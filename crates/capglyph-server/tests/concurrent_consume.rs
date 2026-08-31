use std::sync::Arc;
use std::thread;

use capglyph_server::db::Db;
use capglyph_server::models::{NewCover, NewCredential};
use capglyph_server::service::{Kms, Service};
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

fn setup_service_with_credential(max_uses: Option<i64>) -> (Service, String, Uuid) {
    // Use temp file DB for real WAL concurrency (not in-memory mutex)
    let (db, _tmp) = Db::new_temp_file().expect("temp db");
    // Need to keep _tmp alive; we leak it for test simplicity by forgetting?
    // Instead we use Arc and keep dir alive via Box::leak.
    // For this helper we will use in-memory shared but spawn threads sharing Arc<Service>.
    // Actually we will create file DB and then clone Service which holds path.
    // Threads will open new connections via path, so concurrency is real.

    // We need to keep TempDir alive; easiest is to create service with file DB and then
    // forget the dir guard (leaks temp file, okay for test).
    std::mem::forget(_tmp);

    let kms = Kms::new().with_key("default", [0x42; 32]);
    let svc = Service::new(db.clone(), kms);

    let cover = svc
        .db
        .create_cover(NewCover {
            sha256: vec![1, 2, 3, 4],
            object_uri: "file://cover.png".into(),
            width: 512,
            height: 512,
            format: "png".into(),
            family_id: None,
            status: "active".into(),
        })
        .unwrap();

    // Create credential with known token
    let mut token_id = [0u8; 16];
    token_id.copy_from_slice(&[
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
        0x00,
    ]);
    let cred = svc
        .db
        .create_credential(NewCredential {
            cover_id: cover.id,
            subject_id: None,
            scope: json!(["download:asset:42"]),
            mode: "dct".into(),
            schema_version: 1,
            key_id: "default".into(),
            embed_params: json!({}),
            output_sha256: vec![0u8; 32],
            token_id,
            not_before: None,
            expires_at: None,
            max_uses,
        })
        .unwrap();

    let token_b64 = capglyph_server::models::token_id_to_base64url(&token_id);
    (svc, token_b64, cred.id)
}

#[test]
fn concurrent_consume_single_use_no_double_spend() {
    let (svc, token, _cred_id) = setup_service_with_credential(Some(1));
    let svc = Arc::new(svc);
    let mut handles = Vec::new();
    let num_threads = 10;

    for i in 0..num_threads {
        let svc_clone = Arc::clone(&svc);
        let token_clone = token.clone();
        handles.push(thread::spawn(move || {
            let idem = format!("idem-{}", i);
            svc_clone.consume(&token_clone, &idem, None)
        }));
    }

    let mut successes = 0;
    let mut failures = 0;
    for h in handles {
        match h.join().unwrap() {
            Ok(_) => successes += 1,
            Err(e) => {
                // Expect Exhausted for losers, or NotFound never
                match e {
                    capglyph_server::ServerError::Exhausted => failures += 1,
                    _ => failures += 1,
                }
            }
        }
    }

    assert_eq!(
        successes, 1,
        "exactly one thread should succeed for max_uses=1"
    );
    assert_eq!(failures, 9, " remaining threads should fail");

    // Verify DB state: use_count == 1
    let cred = svc
        .db
        .get_credential_by_token_hash(&capglyph_server::models::sha256(
            &capglyph_server::models::parse_token_id(&token).unwrap(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(cred.use_count, 1);
}

#[test]
fn concurrent_consume_multi_use_quota() {
    let (svc, token, _cred_id) = setup_service_with_credential(Some(3));
    let svc = Arc::new(svc);
    let mut handles = Vec::new();
    let num_threads = 10;

    for i in 0..num_threads {
        let svc_clone = Arc::clone(&svc);
        let token_clone = token.clone();
        handles.push(thread::spawn(move || {
            let idem = format!("multi-idem-{}", i);
            svc_clone.consume(&token_clone, &idem, None)
        }));
    }

    let mut successes = 0;
    for h in handles {
        if h.join().unwrap().is_ok() {
            successes += 1;
        }
    }

    assert_eq!(successes, 3, "exactly max_uses=3 should succeed");
    let cred = svc
        .db
        .get_credential_by_token_hash(&capglyph_server::models::sha256(
            &capglyph_server::models::parse_token_id(&token).unwrap(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(cred.use_count, 3);
}

#[test]
fn idempotent_replay_does_not_double_spend() {
    let (svc, token, _cred_id) = setup_service_with_credential(Some(5));
    let svc = Arc::new(svc);

    // First consume with idem key
    let res1 = svc.consume(&token, "idem-replay-1", None).unwrap();
    assert_eq!(res1.use_count, 1);

    // Replay same idempotency key should succeed without incrementing
    let res2 = svc.consume(&token, "idem-replay-1", None).unwrap();
    // Our implementation returns current use_count without increment; so still 1
    assert_eq!(res2.use_count, 1);

    // Different key should increment
    let res3 = svc.consume(&token, "idem-replay-2", None).unwrap();
    assert_eq!(res3.use_count, 2);

    let cred = svc
        .db
        .get_credential_by_token_hash(&capglyph_server::models::sha256(
            &capglyph_server::models::parse_token_id(&token).unwrap(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(cred.use_count, 2);
}

#[test]
fn verify_is_readonly_does_not_burn_quota() {
    let (svc, token, _cred_id) = setup_service_with_credential(Some(2));
    // Verify multiple times
    for _ in 0..5 {
        svc.verify(&token).unwrap();
    }
    let cred = svc
        .db
        .get_credential_by_token_hash(&capglyph_server::models::sha256(
            &capglyph_server::models::parse_token_id(&token).unwrap(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(cred.use_count, 0, "verify must not increment use_count");

    // Consume once
    svc.consume(&token, "quota-test-1", None).unwrap();
    let cred = svc
        .db
        .get_credential_by_token_hash(&capglyph_server::models::sha256(
            &capglyph_server::models::parse_token_id(&token).unwrap(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(cred.use_count, 1);
}

#[test]
fn revoked_credential_cannot_be_consumed() {
    let (svc, token, cred_id) = setup_service_with_credential(Some(5));
    svc.db.revoke(&cred_id, None).unwrap();

    // Verify should fail with Revoked
    let v = svc.verify(&token);
    assert!(matches!(v, Err(capglyph_server::ServerError::Revoked)));

    // Consume should fail with Revoked
    let c = svc.consume(&token, "idem-after-revoke", None);
    assert!(matches!(c, Err(capglyph_server::ServerError::Revoked)));

    // Audit event should exist
    let events = svc.db.list_audit_events(Some(cred_id), 10).unwrap();
    assert!(events.iter().any(|e| e.event_type == "credential.revoked"));
}

#[test]
fn expired_credential_cannot_be_consumed() {
    let (db, _tmp) = Db::new_temp_file().unwrap();
    std::mem::forget(_tmp);
    let kms = Kms::new().with_key("default", [0x42; 32]);
    let svc = Service::new(db, kms);
    let cover = svc
        .db
        .create_cover(NewCover {
            sha256: vec![5, 6, 7],
            object_uri: "file://cover2.png".into(),
            width: 512,
            height: 512,
            format: "png".into(),
            family_id: None,
            status: "active".into(),
        })
        .unwrap();
    let mut token_id = [0u8; 16];
    token_id.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    let past = Utc::now() - Duration::hours(1);
    let _cred = svc
        .db
        .create_credential(NewCredential {
            cover_id: cover.id,
            subject_id: None,
            scope: json!([]),
            mode: "dct".into(),
            schema_version: 1,
            key_id: "default".into(),
            embed_params: json!({}),
            output_sha256: vec![0u8; 32],
            token_id,
            not_before: None,
            expires_at: Some(past),
            max_uses: Some(10),
        })
        .unwrap();
    let token_b64 = capglyph_server::models::token_id_to_base64url(&token_id);
    let v = svc.verify(&token_b64);
    assert!(matches!(v, Err(capglyph_server::ServerError::Expired)));
    let c = svc.consume(&token_b64, "idem-expired", None);
    assert!(matches!(c, Err(capglyph_server::ServerError::Expired)));
}

#[test]
fn audit_trail_records_issue_consume_revoke() {
    let (svc, token, cred_id) = setup_service_with_credential(Some(2));
    // consume
    svc.consume(&token, "audit-idem-1", None).unwrap();
    // revoke
    svc.db.revoke(&cred_id, None).unwrap();

    let events = svc.db.list_audit_events(Some(cred_id), 20).unwrap();
    let types: Vec<_> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert!(types.contains(&"credential.issued"));
    assert!(types.contains(&"credential.consumed"));
    assert!(types.contains(&"credential.revoked"));
}

#[test]
fn carrier_framing_integration_roundtrip() {
    use capglyph_server::carrier_integration::{decode_credential_token, encode_credential_token};
    let k_mac = [0x77u8; 32];
    let token = [0x11u8; 16];
    let coded = encode_credential_token(&token, &k_mac);
    let decoded = decode_credential_token(&coded, &k_mac).unwrap();
    assert_eq!(decoded, token);
}
