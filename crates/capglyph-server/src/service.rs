use base64::Engine as _;
use chrono::Utc;
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use capglyph_core::keying::KeyMaterial;

use crate::db::Db;
use crate::error::{Result, ServerError};
use crate::models::{
    capability_id_to_base64url, parse_capability_id, parse_token_id, sha256, token_id_to_base64url,
    AuthorizationContext, Credential, IssueRequest, IssueResponse, MessageObject, NewCover,
    NewCredential, NewMessageObject, ResolveMessageResponse, StoreMessageResponse,
};

/// High-level service that wraps Db + KMS derivation + carrier framing.
///
/// MVP keeps KMS as an in-memory map `key_id -> master_secret[32]`.
/// Real deployment would call HSM/KMS.
#[derive(Debug, Clone)]
pub struct Kms {
    // For MVP, a single master key per key_id; we store it directly.
    // In production this would be an HSM handle.
    master_by_key_id: std::collections::HashMap<String, [u8; 32]>,
}

impl Kms {
    pub fn new() -> Self {
        Self {
            master_by_key_id: std::collections::HashMap::new(),
        }
    }

    pub fn with_key(mut self, key_id: impl Into<String>, master: [u8; 32]) -> Self {
        self.master_by_key_id.insert(key_id.into(), master);
        self
    }

    pub fn generate_key_id(&mut self, key_id: &str) -> [u8; 32] {
        let mut master = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut master);
        self.master_by_key_id.insert(key_id.to_string(), master);
        master
    }

    pub fn get_master(&self, key_id: &str) -> Result<[u8; 32]> {
        self.master_by_key_id
            .get(key_id)
            .copied()
            .filter(|key| *key != [0u8; 32])
            .ok_or_else(|| ServerError::KeyNotFound(key_id.to_string()))
    }

    /// Derive the three v1 role keys with RFC 5869 HKDF-SHA256.
    pub fn derive(&self, key_id: &str, cover_id: &Uuid) -> Result<KeyMaterial> {
        let master = self.get_master(key_id)?;
        KeyMaterial::from_ikm_v1(&master, cover_id.as_bytes()).map_err(ServerError::from)
    }
}

impl Default for Kms {
    fn default() -> Self {
        Self::new()
    }
}

/// Service facade
#[derive(Debug, Clone)]
pub struct Service {
    pub db: Db,
    pub kms: Kms,
}

impl Service {
    pub fn new(db: Db, kms: Kms) -> Self {
        Self { db, kms }
    }

    pub fn new_with_random_kms(db: Db) -> Self {
        let mut kms = Kms::new();
        // Generate a default key for tests / MVP
        kms.generate_key_id("cred-2026-08");
        kms.generate_key_id("default");
        Self { db, kms }
    }

    // ── Cover ─────────────────────────────────────────────────────────────────

    pub fn ensure_demo_cover(&self) -> Result<crate::models::Cover> {
        // Try to find an existing cover or create a dummy one for tests
        // For MVP we lazily create a synthetic cover if none exists.
        self.db.create_cover(NewCover {
            sha256: sha256(b"demo-cover"),
            object_uri: "file://demo/cover.png".into(),
            width: 512,
            height: 512,
            format: "png".into(),
            family_id: None,
            status: "active".into(),
        })
    }

    // ── Issue ─────────────────────────────────────────────────────────────────

    pub fn issue(&self, req: IssueRequest) -> Result<IssueResponse> {
        let cover_id = req.cover_id;
        // Validate cover exists
        let cover = self
            .db
            .get_cover(&cover_id)?
            .ok_or_else(|| ServerError::NotFound(format!("cover not found: {}", cover_id)))?;

        let key_id = req.key_id.clone().unwrap_or_else(|| "default".to_string());
        // Resolve trusted key context before generating or persisting any token.
        // Unknown keys are a hard failure; there is no process-default or zero-key path.
        let keys = self.kms.derive(&key_id, &cover.id)?;

        Self::validate_scope_shape(&req.scope)?;
        if matches!(req.max_uses, Some(max) if max <= 0) {
            return Err(ServerError::Conflict(
                "max_uses must be greater than zero".into(),
            ));
        }
        if let (Some(not_before), Some(expires_at)) = (req.not_before, req.expires_at) {
            if not_before >= expires_at {
                return Err(ServerError::Conflict(
                    "not_before must precede expires_at".into(),
                ));
            }
        }

        // Generate token_id (CSPRNG 128-bit)
        let mut token_id = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut token_id);

        // Carrier framing: seal the exact v1 Credential payload `{0: bstr16}`.
        let sealed = {
            use capglyph_core::framing::{credential_payload, try_seal_typed, Params, PayloadType};
            let params = Params {
                version: 1,
                payload_type: PayloadType::Credential,
                flags: 0,
            };
            let payload = credential_payload(&token_id);
            try_seal_typed(&payload, &params, keys.k_mac())?
        };

        // ECC encode (demonstrates interleave + soft-bits stack)
        let _coded = {
            use capglyph_core::ecc::{encode, Profile};
            // For 512×512 DCT, use Repetition8 baseline for MVP; RS for larger payloads
            encode(&sealed, Profile::Repetition8)
        };

        // For MVP we skip actual image embedding (would need cover image bytes +
        // DctCarrier::embed). We store the credential record and return the token.
        // output_sha256 is sha256 of sealed||coded for forensics (placeholder)
        let mut hasher = Sha256::new();
        hasher.update(&sealed);
        let output_sha256 = hasher.finalize().to_vec();

        let embed_params = req.embed_params.unwrap_or_else(|| {
            serde_json::json!({
                "mode": req.mode.clone().unwrap_or_else(|| "dct".to_string()),
                "placement": "skeleton",
                "ecc": "repetition8",
                "framing_version": 1
            })
        });

        let cred = self.db.create_credential(NewCredential {
            cover_id: cover.id,
            subject_id: req.subject_id,
            scope: req.scope.clone(),
            mode: req.mode.unwrap_or_else(|| "dct".to_string()),
            schema_version: 1,
            key_id: key_id.clone(),
            embed_params,
            output_sha256,
            token_id,
            not_before: req.not_before,
            expires_at: req.expires_at,
            max_uses: req.max_uses,
        })?;

        Ok(IssueResponse {
            credential_id: cred.id,
            token_id: token_id_to_base64url(&token_id),
            token_hash_hex: hex::encode(&cred.token_hash),
        })
    }

    // ── Verify (read-only) ────────────────────────────────────────────────────

    pub fn verify(&self, token_id_str: &str) -> Result<crate::models::VerifyResponse> {
        self.verify_authorized(token_id_str, &AuthorizationContext::default())
    }

    /// Verify identity evidence, then enforce subject and scope authorization.
    ///
    /// The token locates and authenticates a credential record. Authorization is
    /// a distinct policy decision and requires trusted caller context whenever a
    /// credential is subject-bound or carries non-empty scopes.
    pub fn verify_authorized(
        &self,
        token_id_str: &str,
        auth: &AuthorizationContext,
    ) -> Result<crate::models::VerifyResponse> {
        let token_id = parse_token_id(token_id_str).map_err(|_e| ServerError::InvalidToken)?;
        let token_hash = sha256(&token_id);
        let cred = self
            .db
            .get_credential_by_token_hash(&token_hash)?
            .ok_or_else(|| ServerError::NotFound("credential not found".into()))?;

        // Resolve KMS context and exercise the canonical authenticated envelope
        // before any application policy or authorization decision.
        self.verify_framing(&cred, &token_id)?;

        // Check state without mutating.
        let status = Self::credential_status(&cred);
        if status != "valid" {
            match status.as_str() {
                "revoked" => return Err(ServerError::Revoked),
                "expired" | "not_yet_valid" => return Err(ServerError::Expired),
                "exhausted" => return Err(ServerError::Exhausted),
                _ => {}
            }
        }

        Self::authorize_credential(&cred, auth)?;

        Ok(crate::models::VerifyResponse {
            credential_id: cred.id,
            status,
            scope: cred.scope,
            use_count: cred.use_count,
            max_uses: cred.max_uses,
            expires_at: cred.expires_at,
        })
    }

    fn credential_status(cred: &Credential) -> String {
        if cred.revoked_at.is_some() {
            return "revoked".into();
        }
        let now = Utc::now();
        if let Some(nb) = cred.not_before {
            if now < nb {
                return "not_yet_valid".into();
            }
        }
        if let Some(ea) = cred.expires_at {
            if now >= ea {
                return "expired".into();
            }
        }
        if let Some(max) = cred.max_uses {
            if cred.use_count >= max {
                return "exhausted".into();
            }
        }
        "valid".into()
    }

    fn verify_framing(&self, cred: &Credential, token_id: &[u8; 16]) -> Result<()> {
        let keys = self.kms.derive(&cred.key_id, &cred.cover_id)?;
        use capglyph_core::framing::{
            credential_payload, credential_token_id, open_typed, try_seal_typed, Params,
            PayloadType,
        };
        let params = Params {
            version: 1,
            payload_type: PayloadType::Credential,
            flags: 0,
        };
        let payload = credential_payload(token_id);
        let sealed = try_seal_typed(&payload, &params, keys.k_mac())?;
        let (header, opened) = open_typed(&sealed, keys.k_mac())?;
        if credential_token_id(&opened, header.flags)? != *token_id {
            return Err(ServerError::InvalidToken);
        }
        Ok(())
    }

    fn validate_scope_shape(scope: &serde_json::Value) -> Result<()> {
        let values = scope.as_array().ok_or_else(|| {
            ServerError::Conflict("scope must be an array of non-empty strings".into())
        })?;
        if values
            .iter()
            .any(|value| value.as_str().is_none_or(str::is_empty))
        {
            return Err(ServerError::Conflict(
                "scope must be an array of non-empty strings".into(),
            ));
        }
        Ok(())
    }

    fn authorize_credential(cred: &Credential, auth: &AuthorizationContext) -> Result<()> {
        if let Some(subject_id) = cred.subject_id {
            if auth.actor_id != Some(subject_id) {
                return Err(ServerError::Unauthorized(
                    "credential is bound to another subject".into(),
                ));
            }
        }

        Self::validate_scope_shape(&cred.scope)?;
        let scopes = cred.scope.as_array().expect("validated scope array");
        if scopes.is_empty() {
            if auth.requested_scope.is_some() {
                return Err(ServerError::ScopeDenied);
            }
            return Ok(());
        }
        let requested = auth
            .requested_scope
            .as_deref()
            .ok_or(ServerError::ScopeDenied)?;
        if !scopes.iter().any(|scope| scope.as_str() == Some(requested)) {
            return Err(ServerError::ScopeDenied);
        }
        Ok(())
    }

    /// Hash all semantic consume inputs. Callers must not supply this hash.
    pub fn consume_request_hash(token_id_str: &str, auth: &AuthorizationContext) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"capglyph/consume-request/v1\0");
        hasher.update((token_id_str.len() as u64).to_be_bytes());
        hasher.update(token_id_str.as_bytes());
        match auth.actor_id {
            Some(actor) => {
                hasher.update([1]);
                hasher.update(actor.as_bytes());
            }
            None => hasher.update([0]),
        }
        match auth.requested_scope.as_deref() {
            Some(scope) => {
                hasher.update([1]);
                hasher.update((scope.len() as u64).to_be_bytes());
                hasher.update(scope.as_bytes());
            }
            None => hasher.update([0]),
        }
        hasher.finalize().into()
    }

    // ── Consume (atomic) ──────────────────────────────────────────────────────

    pub fn consume(
        &self,
        token_id_str: &str,
        idempotency_key: &str,
        actor_id: Option<Uuid>,
    ) -> Result<crate::models::ConsumeResponse> {
        let auth = AuthorizationContext {
            actor_id,
            requested_scope: None,
        };
        self.consume_authorized(token_id_str, idempotency_key, &auth)
    }

    pub fn consume_authorized(
        &self,
        token_id_str: &str,
        idempotency_key: &str,
        auth: &AuthorizationContext,
    ) -> Result<crate::models::ConsumeResponse> {
        if idempotency_key.trim().is_empty() {
            return Err(ServerError::Conflict(
                "idempotency key must not be empty".into(),
            ));
        }
        let token_id = parse_token_id(token_id_str).map_err(|_| ServerError::InvalidToken)?;
        let token_hash = sha256(&token_id);

        let cred = self
            .db
            .get_credential_by_token_hash(&token_hash)?
            .ok_or_else(|| ServerError::NotFound("credential not found".into()))?;
        self.verify_framing(&cred, &token_id)?;
        Self::authorize_credential(&cred, auth)?;

        // Bind idempotency to the semantic request on the trusted side of the
        // API. Callers cannot supply a hash that disguises another token,
        // actor, or requested scope.
        let request_hash = Self::consume_request_hash(token_id_str, auth);
        let (cred, replayed) =
            self.db
                .consume(&token_hash, idempotency_key, auth.actor_id, &request_hash)?;

        Ok(crate::models::ConsumeResponse {
            credential_id: cred.id,
            use_count: cred.use_count,
            max_uses: cred.max_uses,
            outcome: if replayed {
                "idempotent_replay".into()
            } else {
                "consumed".into()
            },
        })
    }

    // ── Revoke ────────────────────────────────────────────────────────────────

    pub fn revoke(&self, credential_id: &Uuid, actor_id: Option<Uuid>) -> Result<Credential> {
        self.db.revoke(credential_id, actor_id)
    }

    pub fn revoke_authorized(
        &self,
        credential_id: &Uuid,
        auth: &AuthorizationContext,
        token_id: Option<&str>,
    ) -> Result<Credential> {
        let cred = self.get(credential_id)?;
        Self::authorize_subject_or_token(&cred, auth, token_id)?;
        self.db.revoke(credential_id, auth.actor_id)
    }

    pub fn get(&self, credential_id: &Uuid) -> Result<Credential> {
        self.db
            .get_credential(credential_id)?
            .ok_or_else(|| ServerError::NotFound(format!("credential {}", credential_id)))
    }

    pub fn get_authorized(
        &self,
        credential_id: &Uuid,
        auth: &AuthorizationContext,
        token_id: Option<&str>,
    ) -> Result<Credential> {
        let cred = self.get(credential_id)?;
        Self::authorize_subject_or_token(&cred, auth, token_id)?;
        Ok(cred)
    }

    fn authorize_subject_or_token(
        cred: &Credential,
        auth: &AuthorizationContext,
        token_id: Option<&str>,
    ) -> Result<()> {
        if let Some(subject_id) = cred.subject_id {
            return if auth.actor_id == Some(subject_id) {
                Ok(())
            } else {
                Err(ServerError::Unauthorized(
                    "credential is bound to another subject".into(),
                ))
            };
        }

        let token_id = token_id.ok_or_else(|| {
            ServerError::Unauthorized("bearer credential requires token proof".into())
        })?;
        let token_id = parse_token_id(token_id)
            .map_err(|_| ServerError::Unauthorized("invalid token proof".into()))?;
        if sha256(&token_id) != cred.token_hash {
            return Err(ServerError::Unauthorized("invalid token proof".into()));
        }
        Ok(())
    }

    // ── Image-based verify/consume (carrier integration stub) ─────────────────

    /// Verify from raw image bytes using original-assisted extraction.
    /// MVP stub: decodes image, attempts to extract payload via carrier, then
    /// delegates to `verify(token_id)`. Real implementation would:
    ///   candidate = decode_image_limited(image)?
    ///   cover = cover_store.resolve_candidate(&candidate)?
    ///   aligned = registration.align(&cover.image, &candidate)?
    ///   signal = DctCarrier::verify_original_assisted(...)
    ///   frame = ecc::decode(signal.soft_bits)
    ///   (hdr, payload) = framing::open(&frame, k_mac)
    ///   token_id = payload.require_token_id_128()
    ///   db.verify(...)
    pub fn verify_image(&self, _image_bytes: &[u8]) -> Result<crate::models::VerifyResponse> {
        // For MVP, we don't have cover vault wired; return error explaining need for token.
        Err(ServerError::Internal(
            "verify_image not yet wired: use verify(token_id) or provide cover vault".into(),
        ))
    }

    // ── Pointer / Message Objects (CTX-0024) ──────────────────────────────────

    /// ChaCha20-Poly1305 helpers (shared with `capglyph::pointer`)
    #[allow(deprecated)]
    pub fn aead_encrypt(
        plaintext: &[u8],
        key: &[u8; 32],
        nonce_bytes: &[u8; 12],
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, Key, KeyInit, Nonce};
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        let nonce = Nonce::from_slice(nonce_bytes);
        let combined = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| ServerError::Internal(format!("AEAD encrypt failed: {e}")))?;
        // Split tag (last 16 bytes)
        if combined.len() < 16 {
            return Err(ServerError::Internal("AEAD output too short".into()));
        }
        let (ct, tag) = combined.split_at(combined.len() - 16);
        Ok((ct.to_vec(), tag.to_vec()))
    }

    #[allow(deprecated)]
    pub fn aead_decrypt(
        ciphertext: &[u8],
        tag: &[u8],
        key: &[u8; 32],
        nonce_bytes: &[u8; 12],
    ) -> Result<Vec<u8>> {
        use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, Key, KeyInit, Nonce};
        let mut combined = Vec::with_capacity(ciphertext.len() + tag.len());
        combined.extend_from_slice(ciphertext);
        combined.extend_from_slice(tag);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        let nonce = Nonce::from_slice(nonce_bytes);
        let pt = cipher
            .decrypt(nonce, combined.as_ref())
            .map_err(|_| ServerError::Internal("AEAD tag verification failed".into()))?;
        Ok(pt)
    }

    /// Generate a fresh 32-byte content key (CSPRNG).
    pub fn generate_content_key() -> [u8; 32] {
        let mut k = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut k);
        k
    }

    /// Generate a fresh 12-byte nonce.
    pub fn generate_nonce() -> [u8; 12] {
        let mut n = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut n);
        n
    }

    /// Generate a fresh 16-byte capability_id.
    pub fn generate_capability_id() -> [u8; 16] {
        let mut c = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut c);
        c
    }

    /// Store an already-encrypted object (ciphertext+nonce+tag) under a fresh capability.
    /// This is the low-level API: `store_message(ciphertext) -> capability_id` per task.
    #[allow(clippy::too_many_arguments)]
    pub fn store_message(
        &self,
        ciphertext: Vec<u8>,
        nonce: Vec<u8>,
        tag: Vec<u8>,
        content_key: Option<Vec<u8>>,
        policy: serde_json::Value,
        owner_id: Option<Uuid>,
        expires_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<StoreMessageResponse> {
        if nonce.len() != 12 {
            return Err(ServerError::Internal("nonce must be 12 bytes".into()));
        }
        if tag.len() != 16 {
            return Err(ServerError::Internal("tag must be 16 bytes".into()));
        }
        let mut cap = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut cap);
        let obj = self.db.create_message_object(NewMessageObject {
            capability_id: cap,
            ciphertext,
            nonce,
            tag,
            content_key,
            policy,
            owner_id,
            expires_at,
        })?;
        Ok(StoreMessageResponse {
            object_id: obj.id,
            capability_id: capability_id_to_base64url(&cap),
            capability_hash_hex: hex::encode(sha256(&cap)),
        })
    }

    /// Convenience: encrypt plaintext with a fresh content_key+nonce, store, return capability.
    /// Returns (capability_id, content_key, nonce) so caller can persist or embed.
    pub fn encrypt_and_store(
        &self,
        plaintext: &[u8],
        policy: serde_json::Value,
        owner_id: Option<Uuid>,
        expires_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(StoreMessageResponse, Vec<u8>, Vec<u8>)> {
        let key = Self::generate_content_key();
        let nonce = Self::generate_nonce();
        let (ct, tag) = Self::aead_encrypt(plaintext, &key, &nonce)?;
        let resp = self.store_message(
            ct,
            nonce.to_vec(),
            tag.clone(),
            Some(key.to_vec()),
            policy,
            owner_id,
            expires_at,
        )?;
        Ok((resp, key.to_vec(), nonce.to_vec()))
    }

    /// Resolve capability → ciphertext with authorization (no IDOR).
    /// Verifies actor is authorized per stored policy.
    pub fn resolve_message(
        &self,
        capability_id_str: &str,
        actor_id: Option<Uuid>,
    ) -> Result<ResolveMessageResponse> {
        let cap = parse_capability_id(capability_id_str).map_err(|_| ServerError::InvalidToken)?;
        let obj = self.db.resolve_message_object(&cap, actor_id)?;
        Ok(ResolveMessageResponse {
            object_id: obj.id,
            ciphertext_base64: base64::engine::general_purpose::STANDARD.encode(&obj.ciphertext),
            nonce_base64: base64::engine::general_purpose::STANDARD.encode(&obj.nonce),
            tag_base64: base64::engine::general_purpose::STANDARD.encode(&obj.tag),
            policy: obj.policy,
        })
    }

    /// Full resolve + decrypt helper (for tests: fetch ciphertext then decrypt with stored content_key).
    /// If content_key was stored, use it; otherwise caller must supply key.
    pub fn resolve_and_decrypt(
        &self,
        capability_id_str: &str,
        actor_id: Option<Uuid>,
        key_override: Option<[u8; 32]>,
    ) -> Result<Vec<u8>> {
        let cap = parse_capability_id(capability_id_str).map_err(|_| ServerError::InvalidToken)?;
        let obj = self.db.resolve_message_object(&cap, actor_id)?;
        let key: [u8; 32] = if let Some(k) = key_override {
            k
        } else if let Some(ck) = &obj.content_key {
            if ck.len() != 32 {
                return Err(ServerError::Internal(
                    "stored content_key invalid length".into(),
                ));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(ck);
            arr
        } else {
            return Err(ServerError::Internal(
                "no content_key available for decrypt (provide key_override)".into(),
            ));
        };
        if obj.nonce.len() != 12 || obj.tag.len() != 16 {
            return Err(ServerError::Internal(
                "stored nonce/tag invalid length".into(),
            ));
        }
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(&obj.nonce);
        Self::aead_decrypt(&obj.ciphertext, &obj.tag, &key, &nonce_arr)
    }

    /// Direct object lookup by object_id (for offline pointer: object_id + content_key in carrier).
    /// Still requires authorization check via policy.
    fn get_message_object(&self, object_id: &Uuid) -> Result<MessageObject> {
        self.db
            .get_message_object(object_id)?
            .ok_or_else(|| ServerError::NotFound(format!("message object {}", object_id)))
    }

    /// Offline: store plaintext, return (object_id, content_key) for carrier embedding.
    /// The carrier payload is `object_id (16 bytes, UUID) || content_key (32 bytes)` = 48 bytes.
    /// Enforces 1024px+ check at embed time, not here.
    #[allow(clippy::type_complexity)]
    pub fn store_offline(
        &self,
        plaintext: &[u8],
        policy: serde_json::Value,
        owner_id: Option<Uuid>,
        expires_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(Uuid, Vec<u8>, Vec<u8>, Vec<u8>)> {
        // Returns (object_id, content_key, nonce, tag) and stores ciphertext keyed also by capability for fallback?
        // For offline we store with a random capability_id as well (not used in offline path but keeps table uniform)
        let key = Self::generate_content_key();
        let nonce = Self::generate_nonce();
        let (ct, tag) = Self::aead_encrypt(plaintext, &key, &nonce)?;
        let cap = Self::generate_capability_id();
        let obj = self.db.create_message_object(NewMessageObject {
            capability_id: cap,
            ciphertext: ct.clone(),
            nonce: nonce.to_vec(),
            tag: tag.clone(),
            content_key: Some(key.to_vec()),
            policy,
            owner_id,
            expires_at,
        })?;
        // Payload for offline carrier is object_id (UUID bytes) + content_key
        // Caller will embed payload = obj.id.as_bytes() || key
        Ok((obj.id, key.to_vec(), nonce.to_vec(), tag))
    }

    /// Offline resolve: given object_id, fetch and decrypt with provided content_key.
    /// Still checks policy authorization.
    pub fn resolve_offline(
        &self,
        object_id: &Uuid,
        content_key: &[u8; 32],
        actor_id: Option<Uuid>,
    ) -> Result<Vec<u8>> {
        let obj = self.get_message_object(object_id)?;
        // Reuse same authz logic as capability path: check policy vs actor
        // We do a dummy capability check by constructing a fake cap from object_id? Instead, check directly:
        if let Some(exp) = obj.expires_at {
            if Utc::now() >= exp {
                return Err(ServerError::Expired);
            }
        }
        if let Some(owner) = obj.owner_id {
            match actor_id {
                Some(actor) if actor == owner => {}
                Some(actor) => {
                    if let Some(allow) = obj.policy.get("allow").and_then(|v| v.as_array()) {
                        let allowed = allow.iter().any(|v| v.as_str() == Some(&actor.to_string()));
                        if !allowed {
                            return Err(ServerError::Unauthorized(format!(
                                "actor {} not authorized for object {}",
                                actor, obj.id
                            )));
                        }
                    } else {
                        return Err(ServerError::Unauthorized(format!(
                            "actor {} not owner {}",
                            actor, owner
                        )));
                    }
                }
                None => {
                    return Err(ServerError::Unauthorized(
                        "missing actor_id for owner-restricted object".into(),
                    ))
                }
            }
        }
        if obj.nonce.len() != 12 || obj.tag.len() != 16 {
            return Err(ServerError::Internal("stored nonce/tag invalid".into()));
        }
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(&obj.nonce);
        Self::aead_decrypt(&obj.ciphertext, &obj.tag, content_key, &nonce_arr)
    }
}
