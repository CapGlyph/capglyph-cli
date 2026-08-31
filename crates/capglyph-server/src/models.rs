use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Covers ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cover {
    pub id: Uuid,
    pub sha256: Vec<u8>,
    pub object_uri: String,
    pub width: i32,
    pub height: i32,
    pub format: String,
    pub family_id: Option<Uuid>,
    pub issuance_count: i64,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCover {
    pub sha256: Vec<u8>,
    pub object_uri: String,
    pub width: i32,
    pub height: i32,
    pub format: String,
    pub family_id: Option<Uuid>,
    pub status: String,
}

// ── Credentials ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Credential {
    pub id: Uuid,
    pub token_hash: Vec<u8>,
    pub cover_id: Uuid,
    pub subject_id: Option<Uuid>,
    pub scope: serde_json::Value,
    pub mode: String,
    pub schema_version: i32,
    pub key_id: String,
    pub embed_params: serde_json::Value,
    pub output_sha256: Vec<u8>,
    pub not_before: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_uses: Option<i64>,
    pub use_count: i64,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCredential {
    pub cover_id: Uuid,
    pub subject_id: Option<Uuid>,
    pub scope: serde_json::Value,
    pub mode: String,
    pub schema_version: i32,
    pub key_id: String,
    pub embed_params: serde_json::Value,
    pub output_sha256: Vec<u8>,
    /// 16-byte raw token (CSPRNG). Stored hashed only.
    pub token_id: [u8; 16],
    pub not_before: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_uses: Option<i64>,
}

// ── Consumptions ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialConsumption {
    pub id: Uuid,
    pub credential_id: Uuid,
    pub idempotency_key: String,
    pub actor_id: Option<Uuid>,
    pub consumed_at: DateTime<Utc>,
    pub request_hash: Option<Vec<u8>>,
    pub outcome: String,
}

// ── Audit events ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEvent {
    pub id: Uuid,
    pub event_type: String,
    pub object_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub event_data: Option<serde_json::Value>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAuditEvent {
    pub event_type: String,
    pub object_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub event_data: Option<serde_json::Value>,
}

// ── Service DTOs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRequest {
    pub token_id: String, // base64url or hex 32 chars
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub credential_id: Uuid,
    pub status: String, // "valid" | "expired" | "revoked" | "exhausted"
    pub scope: serde_json::Value,
    pub use_count: i64,
    pub max_uses: Option<i64>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumeRequest {
    pub token_id: String,
    pub idempotency_key: String,
    pub actor_id: Option<Uuid>,
    pub request_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumeResponse {
    pub credential_id: Uuid,
    pub use_count: i64,
    pub max_uses: Option<i64>,
    pub outcome: String, // "consumed" | "idempotent_replay" | error
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeRequest {
    pub actor_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueRequest {
    pub cover_id: Uuid,
    pub scope: serde_json::Value,
    pub mode: Option<String>,
    pub subject_id: Option<Uuid>,
    pub max_uses: Option<i64>,
    pub expires_at: Option<DateTime<Utc>>,
    pub not_before: Option<DateTime<Utc>>,
    pub key_id: Option<String>,
    pub embed_params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueResponse {
    pub credential_id: Uuid,
    pub token_id: String, // base64url — only returned at issuance
    pub token_hash_hex: String,
}

// ── Message Objects (CTX-0024 pointer mode) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageObject {
    pub id: Uuid,
    pub capability_id: Vec<u8>,   // 16-byte bearer token (raw)
    pub capability_hash: Vec<u8>, // SHA-256 of capability_id for indexed lookup
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,               // 12 bytes ChaCha20Poly1305
    pub tag: Vec<u8>,                 // 16 bytes
    pub content_key: Option<Vec<u8>>, // 32 bytes, stored for offline re-derive / audit
    pub policy: serde_json::Value,
    pub owner_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMessageObject {
    pub capability_id: [u8; 16],
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub tag: Vec<u8>,
    pub content_key: Option<Vec<u8>>,
    pub policy: serde_json::Value,
    pub owner_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMessageRequest {
    pub plaintext_base64: String, // base64 of plaintext (client encrypts locally, but server stores ciphertext)
    // Alternatively raw ciphertext fields for direct store
    pub ciphertext_base64: Option<String>,
    pub nonce_base64: Option<String>,
    pub tag_base64: Option<String>,
    pub policy: Option<serde_json::Value>,
    pub owner_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMessageResponse {
    pub object_id: Uuid,
    pub capability_id: String, // base64url
    pub capability_hash_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveMessageRequest {
    pub capability_id: String, // base64url 16 bytes
    pub actor_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveMessageResponse {
    pub object_id: Uuid,
    pub ciphertext_base64: String,
    pub nonce_base64: String,
    pub tag_base64: String,
    pub policy: serde_json::Value,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn parse_token_id(s: &str) -> anyhow::Result<[u8; 16]> {
    // Try base64url, then hex, then raw utf8 (for tests)
    if let Ok(bytes) =
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, s.trim())
    {
        if bytes.len() == 16 {
            let mut out = [0u8; 16];
            out.copy_from_slice(&bytes);
            return Ok(out);
        }
    }
    if let Ok(bytes) = hex::decode(s.trim()) {
        if bytes.len() == 16 {
            let mut out = [0u8; 16];
            out.copy_from_slice(&bytes);
            return Ok(out);
        }
    }
    // Try base64 standard
    if let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s.trim())
    {
        if bytes.len() == 16 {
            let mut out = [0u8; 16];
            out.copy_from_slice(&bytes);
            return Ok(out);
        }
    }
    anyhow::bail!(
        "invalid token_id encoding (expected 16 bytes base64url or hex): {}",
        s
    );
}

pub fn token_id_to_base64url(token: &[u8; 16]) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, token)
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().to_vec()
}

pub fn parse_capability_id(s: &str) -> anyhow::Result<[u8; 16]> {
    parse_token_id(s)
}

pub fn capability_id_to_base64url(cap: &[u8; 16]) -> String {
    token_id_to_base64url(cap)
}
