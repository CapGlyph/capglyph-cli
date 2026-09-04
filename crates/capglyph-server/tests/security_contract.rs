use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use base64::Engine as _;
use capglyph_core::{
    framing::{credential_payload, try_seal_typed, Params, PayloadType},
    keying::KeyMaterial,
};
use capglyph_server::{
    models::{
        parse_token_id, AuthorizationContext, IssueRequest, IssueResponse, NewCover,
        StoreMessageResponse,
    },
    router, Db, Kms, ServerError, Service,
};
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use uuid::Uuid;

const MASTER: [u8; 32] = [0x42; 32];

fn test_service() -> (Service, Uuid) {
    let db = Db::new_in_memory_shared().expect("in-memory database");
    let kms = Kms::new().with_key("default", MASTER);
    let svc = Service::new(db, kms);
    let cover = svc
        .db
        .create_cover(NewCover {
            sha256: vec![0x41; 32],
            object_uri: "memory://security-contract-cover".into(),
            width: 512,
            height: 512,
            format: "png".into(),
            family_id: None,
            status: "active".into(),
        })
        .expect("cover");
    (svc, cover.id)
}

fn issue(
    svc: &Service,
    cover_id: Uuid,
    subject_id: Option<Uuid>,
    scope: Value,
    max_uses: Option<i64>,
    not_before: Option<chrono::DateTime<Utc>>,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> IssueResponse {
    svc.issue(IssueRequest {
        cover_id,
        scope,
        mode: Some("dct".into()),
        subject_id,
        max_uses,
        expires_at,
        not_before,
        key_id: Some("default".into()),
        embed_params: None,
    })
    .expect("issue credential")
}

fn json_request(
    method: Method,
    uri: impl AsRef<str>,
    body: Value,
    headers: &[(&str, String)],
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.as_ref())
        .header("content-type", "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    builder.body(Body::from(body.to_string())).expect("request")
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

async fn post_issue(app: &Router, cover_id: Uuid, body: Value) -> IssueResponse {
    let mut body = body;
    body["cover_id"] = json!(cover_id);
    let response = app
        .clone()
        .oneshot(json_request(Method::POST, "/v1/credentials", body, &[]))
        .await
        .expect("issue response");
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_value(response_json(response).await).expect("issue body")
}

#[test]
fn kms_rejects_missing_and_zero_keys_and_uses_the_normative_schedule() {
    let cover_id = Uuid::from_bytes([0x24; 16]);
    let missing = Kms::new().derive("missing", &cover_id).unwrap_err();
    assert!(matches!(missing, ServerError::KeyNotFound(_)));

    let zero = Kms::new()
        .with_key("zero", [0; 32])
        .derive("zero", &cover_id)
        .unwrap_err();
    assert!(matches!(zero, ServerError::KeyNotFound(_)));

    let derived = Kms::new()
        .with_key("known", MASTER)
        .derive("known", &cover_id)
        .expect("derive configured key");
    let expected = KeyMaterial::from_ikm_v1(&MASTER, cover_id.as_bytes())
        .expect("normative RFC 5869 schedule");
    assert_eq!(derived.k_embed(), expected.k_embed());
    assert_eq!(derived.k_mac(), expected.k_mac());
    assert_eq!(derived.k_object(), expected.k_object());
}

#[test]
fn issue_fails_on_unknown_key_and_seals_the_exact_credential_payload() {
    let (svc, cover_id) = test_service();
    let unknown = svc.issue(IssueRequest {
        cover_id,
        scope: json!([]),
        mode: None,
        subject_id: None,
        max_uses: None,
        expires_at: None,
        not_before: None,
        key_id: Some("missing".into()),
        embed_params: None,
    });
    assert!(matches!(unknown, Err(ServerError::KeyNotFound(_))));

    let issued = issue(&svc, cover_id, None, json!([]), None, None, None);
    let token = parse_token_id(&issued.token_id).expect("token");
    let payload = credential_payload(&token);
    assert_eq!(payload.len(), 19);
    assert_eq!(&payload[..3], &[0xa1, 0x00, 0x50]);

    let params = Params {
        version: 1,
        payload_type: PayloadType::Credential,
        flags: 0,
    };
    let keys = svc.kms.derive("default", &cover_id).unwrap();
    let sealed = try_seal_typed(&payload, &params, keys.k_mac()).unwrap();
    let expected_output_hash = Sha256::digest(sealed).to_vec();
    let stored = svc
        .db
        .get_credential(&issued.credential_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.output_sha256, expected_output_hash);
}

#[test]
fn token_evidence_does_not_bypass_subject_or_scope_authorization() {
    let (svc, cover_id) = test_service();
    let subject = Uuid::new_v4();
    let other = Uuid::new_v4();
    let issued = issue(
        &svc,
        cover_id,
        Some(subject),
        json!(["asset:read"]),
        Some(2),
        None,
        None,
    );

    let wrong_actor = AuthorizationContext {
        actor_id: Some(other),
        requested_scope: Some("asset:read".into()),
    };
    assert!(matches!(
        svc.verify_authorized(&issued.token_id, &wrong_actor),
        Err(ServerError::Unauthorized(_))
    ));

    let missing_scope = AuthorizationContext {
        actor_id: Some(subject),
        requested_scope: None,
    };
    assert!(matches!(
        svc.consume_authorized(&issued.token_id, "denied", &missing_scope),
        Err(ServerError::ScopeDenied)
    ));

    let stored = svc
        .db
        .get_credential(&issued.credential_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.use_count, 0,
        "authorization failure must not burn quota"
    );

    let allowed = AuthorizationContext {
        actor_id: Some(subject),
        requested_scope: Some("asset:read".into()),
    };
    assert_eq!(
        svc.verify_authorized(&issued.token_id, &allowed)
            .unwrap()
            .status,
        "valid"
    );
    assert_eq!(
        svc.consume_authorized(&issued.token_id, "allowed", &allowed)
            .unwrap()
            .use_count,
        1
    );
}

#[test]
fn idempotency_replays_exact_requests_and_rejects_semantic_collisions() {
    let (svc, cover_id) = test_service();
    let issued = issue(
        &svc,
        cover_id,
        None,
        json!(["asset:read", "asset:write"]),
        Some(5),
        None,
        None,
    );
    let read = AuthorizationContext {
        actor_id: None,
        requested_scope: Some("asset:read".into()),
    };
    let first = svc
        .consume_authorized(&issued.token_id, "same-key", &read)
        .unwrap();
    assert_eq!(first.outcome, "consumed");
    let replay = svc
        .consume_authorized(&issued.token_id, "same-key", &read)
        .unwrap();
    assert_eq!(replay.outcome, "idempotent_replay");
    assert_eq!(replay.use_count, 1);

    let write = AuthorizationContext {
        actor_id: None,
        requested_scope: Some("asset:write".into()),
    };
    assert!(matches!(
        svc.consume_authorized(&issued.token_id, "same-key", &write),
        Err(ServerError::Conflict(_))
    ));

    let different_actor = AuthorizationContext {
        actor_id: Some(Uuid::new_v4()),
        requested_scope: Some("asset:read".into()),
    };
    assert!(matches!(
        svc.consume_authorized(&issued.token_id, "same-key", &different_actor),
        Err(ServerError::Conflict(_))
    ));
    let stored = svc
        .db
        .get_credential(&issued.credential_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.use_count, 1);
}

#[test]
fn temporal_revocation_and_quota_checks_are_fail_closed() {
    let (svc, cover_id) = test_service();
    let now = Utc::now();

    let future = issue(
        &svc,
        cover_id,
        None,
        json!([]),
        Some(1),
        Some(now + Duration::hours(1)),
        Some(now + Duration::hours(2)),
    );
    assert!(matches!(
        svc.verify(&future.token_id),
        Err(ServerError::Expired)
    ));
    assert!(matches!(
        svc.consume(&future.token_id, "future", None),
        Err(ServerError::Expired)
    ));

    let expired = issue(
        &svc,
        cover_id,
        None,
        json!([]),
        Some(1),
        None,
        Some(now - Duration::hours(1)),
    );
    assert!(matches!(
        svc.verify(&expired.token_id),
        Err(ServerError::Expired)
    ));

    let revoked = issue(&svc, cover_id, None, json!([]), Some(1), None, None);
    svc.revoke_authorized(
        &revoked.credential_id,
        &AuthorizationContext::default(),
        Some(&revoked.token_id),
    )
    .unwrap();
    assert!(matches!(
        svc.consume(&revoked.token_id, "revoked", None),
        Err(ServerError::Revoked)
    ));

    let quota = issue(&svc, cover_id, None, json!([]), Some(1), None, None);
    svc.consume(&quota.token_id, "quota-1", None).unwrap();
    assert!(matches!(
        svc.consume(&quota.token_id, "quota-2", None),
        Err(ServerError::Exhausted)
    ));
}

#[tokio::test]
async fn api_is_versioned_and_returns_stable_error_envelopes() {
    let (svc, cover_id) = test_service();
    let app = router(svc);

    let version = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(version.status(), StatusCode::OK);
    let version = response_json(version).await;
    assert_eq!(version["api_version"], 1);
    assert_eq!(version["wire_version"], 1);
    assert_eq!(version["spec_version"], "1.0.1");

    let unversioned = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unversioned.status(), StatusCode::NOT_FOUND);

    let unsupported = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/credentials/verify",
            json!({"token_id": "invalid"}),
            &[("accept-capglyph-version", "2".into())],
        ))
        .await
        .unwrap();
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
    let unsupported = response_json(unsupported).await;
    assert_eq!(unsupported["error"], "VERSION_UNSUPPORTED");
    assert_eq!(unsupported["code"], "E_VERSION_UNSUPPORTED");
    assert_eq!(unsupported["supported_versions"], json!([1]));

    let invalid = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/credentials/verify",
            json!({"token_id": "invalid"}),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let invalid = response_json(invalid).await;
    assert_eq!(invalid["error"], "PAYLOAD_INVALID");
    assert_eq!(invalid["code"], "E_PAYLOAD_INVALID");

    let missing_key = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/credentials",
            json!({"cover_id": cover_id, "scope": [], "key_id": "missing"}),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(missing_key.status(), StatusCode::NOT_FOUND);
    let missing_key = response_json(missing_key).await;
    assert_eq!(missing_key["error"], "KEY_NOT_FOUND");
    assert_eq!(missing_key["code"], "E_KEY_NOT_FOUND");
}

#[tokio::test]
async fn credential_http_policy_requires_ingress_identity_and_token_proof() {
    let (svc, cover_id) = test_service();
    let app = router(svc);
    let subject = Uuid::new_v4();
    let issued = post_issue(
        &app,
        cover_id,
        json!({"subject_id": subject, "scope": ["asset:read"], "max_uses": 1}),
    )
    .await;

    // A body actor is merely a claim. Without matching trusted-ingress
    // evidence it is rejected before authorization.
    let self_asserted = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/credentials/consume",
            json!({
                "token_id": issued.token_id,
                "idempotency_key": "self-asserted",
                "actor_id": subject,
                "requested_scope": "asset:read"
            }),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(self_asserted.status(), StatusCode::FORBIDDEN);
    assert_eq!(response_json(self_asserted).await["code"], "E_UNAUTHORIZED");

    let denied_scope = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/credentials/verify",
            json!({"token_id": issued.token_id}),
            &[("x-capglyph-actor-id", subject.to_string())],
        ))
        .await
        .unwrap();
    assert_eq!(denied_scope.status(), StatusCode::FORBIDDEN);
    assert_eq!(response_json(denied_scope).await["code"], "E_SCOPE_DENIED");

    let allowed = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/credentials/consume",
            json!({
                "token_id": issued.token_id,
                "idempotency_key": "allowed",
                "requested_scope": "asset:read"
            }),
            &[("x-capglyph-actor-id", subject.to_string())],
        ))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);

    let exhausted = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/credentials/consume",
            json!({
                "token_id": issued.token_id,
                "idempotency_key": "second",
                "requested_scope": "asset:read"
            }),
            &[("x-capglyph-actor-id", subject.to_string())],
        ))
        .await
        .unwrap();
    assert_eq!(exhausted.status(), StatusCode::CONFLICT);
    assert_eq!(response_json(exhausted).await["code"], "E_CONSUMED");

    let get_without_proof = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/credentials/{}", issued.credential_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_without_proof.status(), StatusCode::FORBIDDEN);

    let get_with_actor = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/credentials/{}", issued.credential_id))
                .header("x-capglyph-actor-id", subject.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_with_actor.status(), StatusCode::OK);
}

#[tokio::test]
async fn message_objects_are_readable_only_through_the_capability_route() {
    let (svc, _cover_id) = test_service();
    let app = router(svc);
    let plaintext = base64::engine::general_purpose::STANDARD.encode(b"secret message");
    let stored = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/messages",
            json!({"plaintext_base64": plaintext, "policy": {}}),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(stored.status(), StatusCode::OK);
    let stored: StoreMessageResponse = serde_json::from_value(response_json(stored).await).unwrap();

    let by_object_id = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/messages/{}", stored.object_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(by_object_id.status(), StatusCode::NOT_FOUND);

    let wrong_capability = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x99; 16]);
    let wrong = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/messages/resolve",
            json!({"capability_id": wrong_capability}),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::NOT_FOUND);
    assert_eq!(response_json(wrong).await["code"], "E_NOT_FOUND");

    let resolved = app
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/v1/messages/resolve",
            json!({"capability_id": stored.capability_id}),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(resolved.status(), StatusCode::OK);
    let resolved = response_json(resolved).await;
    assert_eq!(resolved["object_id"], stored.object_id.to_string());
    assert!(resolved.get("ciphertext_base64").is_some());
    assert!(resolved.get("content_key").is_none());
}
