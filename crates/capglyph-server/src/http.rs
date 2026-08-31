//! HTTP layer for capglyphd — `POST /v1/credentials/{verify,consume,revoke}` + `GET /v1/credentials/{id}`.
//! Only compiled with `http` feature; not pulled into wasm.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use base64::Engine as _;

use crate::error::ServerError;
use crate::models::{
    ConsumeResponse, IssueRequest, IssueResponse, MessageObject, ResolveMessageResponse,
    RevokeRequest, StoreMessageResponse, VerifyResponse,
};
use crate::service::Service;

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub svc: Arc<Service>,
}

// ── Error mapping ─────────────────────────────────────────────────────────────

fn map_err(e: ServerError) -> (StatusCode, Json<serde_json::Value>) {
    let (code, msg) = match &e {
        ServerError::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
        ServerError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
        ServerError::Expired => (StatusCode::GONE, "expired".into()),
        ServerError::Revoked => (StatusCode::GONE, "revoked".into()),
        ServerError::Exhausted => (StatusCode::TOO_MANY_REQUESTS, "exhausted".into()),
        ServerError::InvalidToken => (StatusCode::BAD_REQUEST, "invalid token".into()),
        ServerError::Unauthorized(m) => (StatusCode::FORBIDDEN, m.clone()),
        ServerError::Db(_) | ServerError::Internal(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
        }
    };
    let body = serde_json::json!({ "error": msg, "code": code.as_u16() });
    (code, Json(body))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct VerifyBody {
    pub token_id: String,
}

async fn handle_verify(
    State(state): State<AppState>,
    Json(body): Json<VerifyBody>,
) -> Result<Json<VerifyResponse>, (StatusCode, Json<serde_json::Value>)> {
    state.svc.verify(&body.token_id).map(Json).map_err(map_err)
}

#[derive(Debug, Deserialize)]
pub struct ConsumeBody {
    pub token_id: String,
    pub idempotency_key: Option<String>,
    pub actor_id: Option<Uuid>,
}

async fn handle_consume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ConsumeBody>,
) -> Result<Json<ConsumeResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Idempotency-Key can be in header or body; header takes precedence
    let idem = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or(body.idempotency_key)
        .ok_or_else(|| {
            map_err(ServerError::Internal(
                "missing Idempotency-Key (header or body)".into(),
            ))
        })?;

    state
        .svc
        .consume(&body.token_id, &idem, body.actor_id)
        .map(Json)
        .map_err(map_err)
}

async fn handle_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::models::Credential>, (StatusCode, Json<serde_json::Value>)> {
    state.svc.get(&id).map(Json).map_err(map_err)
}

async fn handle_revoke(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Option<Json<RevokeRequest>>,
) -> Result<Json<crate::models::Credential>, (StatusCode, Json<serde_json::Value>)> {
    // Actor can be in body or header; optional
    let actor_id = body.and_then(|b| b.actor_id).or_else(|| {
        headers
            .get("x-actor-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| Uuid::parse_str(s).ok())
    });
    state.svc.revoke(&id, actor_id).map(Json).map_err(map_err)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IssueBody {
    pub cover_id: Uuid,
    pub scope: serde_json::Value,
    pub mode: Option<String>,
    pub subject_id: Option<Uuid>,
    pub max_uses: Option<i64>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub not_before: Option<chrono::DateTime<chrono::Utc>>,
    pub key_id: Option<String>,
    pub embed_params: Option<serde_json::Value>,
}

async fn handle_issue(
    State(state): State<AppState>,
    Json(body): Json<IssueBody>,
) -> Result<Json<IssueResponse>, (StatusCode, Json<serde_json::Value>)> {
    let req = IssueRequest {
        cover_id: body.cover_id,
        scope: body.scope,
        mode: body.mode,
        subject_id: body.subject_id,
        max_uses: body.max_uses,
        expires_at: body.expires_at,
        not_before: body.not_before,
        key_id: body.key_id,
        embed_params: body.embed_params,
    };
    state.svc.issue(req).map(Json).map_err(map_err)
}

// ── Message objects (CTX-0024) ────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct StoreMessageBody {
    pub plaintext_base64: Option<String>,
    pub ciphertext_base64: Option<String>,
    pub nonce_base64: Option<String>,
    pub tag_base64: Option<String>,
    pub owner_id: Option<Uuid>,
    pub policy: Option<serde_json::Value>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn handle_store_message(
    State(state): State<AppState>,
    Json(body): Json<StoreMessageBody>,
) -> Result<Json<StoreMessageResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Two modes: if plaintext_base64 provided, encrypt server-side; else raw ciphertext
    if let Some(pt_b64) = body.plaintext_base64 {
        let pt = base64::engine::general_purpose::STANDARD
            .decode(pt_b64)
            .map_err(|e| {
                map_err(ServerError::Internal(format!(
                    "invalid plaintext_base64: {e}"
                )))
            })?;
        let policy = body.policy.unwrap_or_else(|| serde_json::json!({}));
        // encrypt_and_store generates key/nonce internally
        state
            .svc
            .encrypt_and_store(&pt, policy, body.owner_id, body.expires_at)
            .map(|(resp, _, _)| Json(resp))
            .map_err(map_err)
    } else {
        let ct = body
            .ciphertext_base64
            .ok_or_else(|| map_err(ServerError::Internal("missing ciphertext_base64".into())))?;
        let nonce_b64 = body
            .nonce_base64
            .ok_or_else(|| map_err(ServerError::Internal("missing nonce_base64".into())))?;
        let tag_b64 = body
            .tag_base64
            .ok_or_else(|| map_err(ServerError::Internal("missing tag_base64".into())))?;
        let ct_bytes = base64::engine::general_purpose::STANDARD
            .decode(ct)
            .map_err(|e| map_err(ServerError::Internal(format!("invalid ciphertext: {e}"))))?;
        let nonce = base64::engine::general_purpose::STANDARD
            .decode(nonce_b64)
            .map_err(|e| map_err(ServerError::Internal(format!("invalid nonce: {e}"))))?;
        let tag = base64::engine::general_purpose::STANDARD
            .decode(tag_b64)
            .map_err(|e| map_err(ServerError::Internal(format!("invalid tag: {e}"))))?;
        let policy = body.policy.unwrap_or_else(|| serde_json::json!({}));
        state
            .svc
            .store_message(
                ct_bytes,
                nonce,
                tag,
                None,
                policy,
                body.owner_id,
                body.expires_at,
            )
            .map(Json)
            .map_err(map_err)
    }
}

#[derive(Debug, Deserialize)]
pub struct ResolveMessageBody {
    pub capability_id: String,
    pub actor_id: Option<Uuid>,
}

async fn handle_resolve_message(
    State(state): State<AppState>,
    Json(body): Json<ResolveMessageBody>,
) -> Result<Json<ResolveMessageResponse>, (StatusCode, Json<serde_json::Value>)> {
    state
        .svc
        .resolve_message(&body.capability_id, body.actor_id)
        .map(Json)
        .map_err(map_err)
}

async fn handle_get_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MessageObject>, (StatusCode, Json<serde_json::Value>)> {
    state.svc.get_message_object(&id).map(Json).map_err(map_err)
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(svc: Service) -> Router {
    let state = AppState { svc: Arc::new(svc) };
    Router::new()
        .route("/v1/credentials", post(handle_issue))
        .route("/v1/credentials/verify", post(handle_verify))
        .route("/v1/credentials/consume", post(handle_consume))
        .route("/v1/credentials/:id", get(handle_get))
        .route("/v1/credentials/:id/revoke", post(handle_revoke))
        .route("/v1/messages", post(handle_store_message))
        .route("/v1/messages/resolve", post(handle_resolve_message))
        .route("/v1/messages/:id", get(handle_get_message))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::models::NewCover;
    use crate::service::{Kms, Service};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // for `oneshot`

    fn test_service() -> Service {
        let db = Db::new_in_memory().unwrap();
        let kms = Kms::new().with_key("default", [0x42; 32]);
        Service::new(db, kms)
    }

    fn test_router() -> Router {
        let svc = test_service();
        // Seed a demo cover for issue tests
        let cover = svc
            .db
            .create_cover(NewCover {
                sha256: vec![1, 2, 3],
                object_uri: "file://test.png".into(),
                width: 512,
                height: 512,
                format: "png".into(),
                family_id: None,
                status: "active".into(),
            })
            .unwrap();
        // Stash cover id via service? We'll just build router normally
        let _ = cover;
        router(svc)
    }

    #[tokio::test]
    async fn verify_not_found_returns_404() {
        let app = test_router();
        let req = Request::builder()
            .uri("/v1/credentials/verify")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"token_id":"AAAAAAAAAAAAAAAAAAAAAA"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn issue_then_verify_then_consume_then_revoke() {
        let svc = test_service();
        let cover = svc
            .db
            .create_cover(NewCover {
                sha256: vec![9, 9, 9],
                object_uri: "file://cover.png".into(),
                width: 512,
                height: 512,
                format: "png".into(),
                family_id: None,
                status: "active".into(),
            })
            .unwrap();
        let app = router(svc);

        // Issue
        let issue_body = serde_json::json!({
            "cover_id": cover.id,
            "scope": ["download:asset:42"],
            "max_uses": 2
        });
        let req = Request::builder()
            .uri("/v1/credentials")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(issue_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 10)
            .await
            .unwrap();
        let issue: IssueResponse = serde_json::from_slice(&body).unwrap();

        // Verify
        let verify_body = serde_json::json!({ "token_id": issue.token_id });
        let req = Request::builder()
            .uri("/v1/credentials/verify")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(verify_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Consume #1
        let consume_body = serde_json::json!({
            "token_id": issue.token_id,
            "idempotency_key": "idem-1"
        });
        let req = Request::builder()
            .uri("/v1/credentials/consume")
            .method("POST")
            .header("content-type", "application/json")
            .header("Idempotency-Key", "idem-1")
            .body(Body::from(consume_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 10)
            .await
            .unwrap();
        let consume: ConsumeResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(consume.use_count, 1);

        // Consume #2 (different idempotency)
        let consume_body2 = serde_json::json!({
            "token_id": issue.token_id,
            "idempotency_key": "idem-2"
        });
        let req = Request::builder()
            .uri("/v1/credentials/consume")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(consume_body2.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Consume #3 should exhaust (max_uses=2)
        let consume_body3 = serde_json::json!({
            "token_id": issue.token_id,
            "idempotency_key": "idem-3"
        });
        let req = Request::builder()
            .uri("/v1/credentials/consume")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(consume_body3.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // Get credential
        let req = Request::builder()
            .uri(format!("/v1/credentials/{}", issue.credential_id))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Revoke (even though exhausted, revoke is idempotent)
        let req = Request::builder()
            .uri(format!("/v1/credentials/{}/revoke", issue.credential_id))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        // First revoke should succeed 200
        assert!(resp.status() == StatusCode::OK || resp.status() == StatusCode::CONFLICT);
    }
}
