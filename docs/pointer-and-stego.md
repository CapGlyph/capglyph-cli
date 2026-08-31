# Pointer-Mode Stego — CTX-0024 Implementation

**Task:** CTX-0024 — Pointer-mode stego: image → capability → encrypted object  
**Branch:** `ctx-0024/feat-pointer`  
**Status:** Implemented (pointer-online default + pointer-offline for 1024px+)

## 1. Three profiles (unchanged from `capglyph-docs/research/media-credential/technology/pointer-and-stego.md` §1)

| Profile           | Image carries                           | Server needed     | Capacity ask    | Implementation                                           |
| ----------------- | --------------------------------------- | ----------------- | --------------- | -------------------------------------------------------- |
| `pointer-online`  | 128-bit capability_id                   | yes               | very low (16 B) | **Default** — bearer capability, server holds ciphertext |
| `pointer-offline` | `object_id (128b) + content_key (256b)` | object store only | 48 B payload    | Mid-tier, 1024×+ carriers, store is dumb                 |
| `direct`          | full AEAD ciphertext                    | no                | high            | Deferred — not in v1                                     |

Credential is `pointer-online` with access-control semantics.

## 2. Pointer-online (implemented)

```
plaintext
  → ChaCha20-Poly1305 (key 32 B, nonce 12 B, tag 16 B)
  → ciphertext stored in `message_objects` (capability_id FK, ciphertext, nonce, tag, policy, owner_id)
  → capability_id (16 B, CSPRNG, base64url) sealed via `capglyph_core::framing` (CBOR + HMAC) + ECC
  → carrier (DCT `F[3,4]` differential or DWT LH) → image
```

Extraction is the inverse plus `soft LLR` decode:

```
stego image → carrier demod → SoftBit LLR → ECC decode → framing open (HMAC verify) → capability_id
           → server `resolve(capability_id, actor_id)` with authz (no IDOR) → ciphertext
           → AEAD decrypt (tag verify) → plaintext
```

- **Framing:** `PayloadType::Pointer` (2), version 1, HMAC via `KeyMaterial.k_mac`
- **ECC:** `Repetition8` for 1024+ DCT / all DWT, `Bch{t=3}` for 512 DCT (insufficient blocks for Repetition8, same as credential ladder)
- **Carrier:** reuses `DctCarrier::embed_coded_bits` / `DwtCarrier::embed_coded_bits` and `extract_coded_bits_soft_with_hint` — same stack as credential
- **Authz (no IDOR):** `message_objects.owner_id` + `policy.allow` list.
  - If `owner_id` set, only that owner or members of `allow` can resolve — wrong actor gets `403 Unauthorized` (tested: `end_to_end_pointer_online` IDOR negative)
  - If no owner and no allow, bearer semantics: any holder of unguessable 128b capability can resolve (capability _is_ authority, but opaque and 2^128)

## 3. Pointer-offline (implemented, 1024px+ gate)

```
plaintext → ChaCha20-Poly1305 → ciphertext (object_id, nonce, tag, content_key) in DB
carrier payload = object_id (16 B UUID BE) || content_key (32 B) = 48 B
  → framing `PayloadType::Locator` + ECC + carrier (1024+ only)
```

- Offline check enforced in `pointer::embed_offline`: `w < 1024 || h < 1024` → `bail!("offline pointer requires image at least 1024x1024")`
- Extraction yields `(object_id, content_key)` directly; `Service::resolve_offline(object_id, content_key, actor_id)` still checks policy (no IDOR) before returning plaintext
- Capacity: 48 B payload → sealed ~86 B → BCH(31,16) ~1333 bits → 2666 block pairs + 512 sync < 4096 (512 DCT) but stealth requires 1024+ per spec §8

## 4. Direct message (deferred)

`plaintext → AEAD → ECC → carrier` without server is a research path; not shipped. The 1024 offline path already covers "long letter" with dumb object store.

## 5. Crypto choices

- **Shared-secret:** `ChaCha20-Poly1305` (RFC 8439) via `chacha20poly1305 0.11` + `aead 0.6`, 12-byte nonce, 16-byte tag. Tag verification is fail-closed (`AEAD tag verification failed`).
- **Asymmetric:** HPKE (RFC 9180) deferred — single-primitive agility per `cryptographic-security.md` §5 (ship ChaCha now, HPKE as next `Sigil-Embed-v2` without stacking)
- **Framing MAC:** HMAC-SHA256 (`KeyMaterial.k_mac`) via `capglyph_core::framing::seal/open` — same as credential, not CRC

## 6. DB schema (CTX-0024)

```sql
CREATE TABLE message_objects (
    id              TEXT PRIMARY KEY, -- UUID object_id
    capability_id   BLOB NOT NULL UNIQUE, -- 16 B bearer
    capability_hash BLOB NOT NULL UNIQUE, -- SHA-256(capability_id) for indexed lookup
    ciphertext      BLOB NOT NULL,
    nonce           BLOB NOT NULL, -- 12 B
    tag             BLOB NOT NULL, -- 16 B
    content_key     BLOB,          -- 32 B (stored for offline re-derive / audit)
    policy          TEXT NOT NULL, -- JSON: {"allow": ["uuid"], ...}
    owner_id        TEXT,
    created_at      TEXT NOT NULL,
    expires_at      TEXT
);
```

- `capability_id` is stored raw (for audit) and hashed for lookup (like `credentials.token_hash` — never log raw bearer)
- `policy` is JSON; `owner_id` is denormalized for fast authz
- Migration: `crates/capglyph-server/migrations/002_pointer.sql` + `db::SCHEMA_SQL` update

## 7. Service API (capglyph-server)

```rust
impl Service {
  fn aead_encrypt(plaintext, key, nonce) -> (ct, tag)
  fn aead_decrypt(ct, tag, key, nonce) -> plaintext // tag verify
  fn store_message(ct, nonce, tag, content_key, policy, owner, expires) -> capability_id
  fn encrypt_and_store(plaintext, policy, owner, expires) -> (capability_id, key, nonce)
  fn resolve_message(capability_id, actor_id) -> ResolveMessageResponse // + authz
  fn resolve_and_decrypt(capability_id, actor_id, key_override) -> plaintext
  fn store_offline(plaintext, policy, owner, expires) -> (object_id, key, nonce, tag)
  fn resolve_offline(object_id, content_key, actor_id) -> plaintext
}
```

All methods are `tokio`-agnostic; HTTP layer (`http.rs`) exposes `POST /v1/messages` and `POST /v1/messages/resolve` (authz maps to `403`).

## 8. Carrier reuse (capglyph crate)

```rust
// capglyph/src/pointer.rs
pub fn embed_online(img, geo, cap, keys, placement, profile, mode)
pub fn extract_online(img, keys, profile, mode) -> cap
pub fn embed_offline(img, geo, object_id, content_key, keys, placement, profile, mode) // 1024 gate
pub fn extract_offline(img, keys, profile, mode) -> (object_id, content_key)
fn select_profile(w, h, mode) -> Profile // same ladder as credential
```

Tests share the same `capglyph_core::{framing,ecc}` + `KeyMaterial` as credential (`framed.rs`).

## 9. CLI (capglyph crate)

```
capglyph pointer embed   --input cover.png --output stego.png --plaintext-file msg.txt --mode dct --db ./capglyph.db --owner-id <uuid>
capglyph pointer extract --input stego.png --mode dct --db ./capglyph.db --actor-id <uuid> --output out.txt
capglyph pointer offline-embed  --input cover1024.png --output stego1024.png --plaintext-file msg.txt --mode dwt --db ./capglyph.db
capglyph pointer offline-extract --input stego1024.png --mode dwt --db ./capglyph.db --actor-id <uuid>

capglyph message encrypt --plaintext-file msg.txt --db ./capglyph.db --owner-id <uuid>  # server only, prints capability_id
capglyph message decrypt --capability-id <b64> --db ./capglyph.db --actor-id <uuid> --output out.txt
capglyph message store/resolve  # same
```

`pointer` is image→capability; `message` is convenience alias (`encrypt`/`decrypt` map to pointer embed/extract, `store`/`resolve` map to server-only).

## 10. Tests (acceptance)

- `pointer::tests::end_to_end_pointer_online` — encrypt→store→embed(1024 DCT)→extract→resolve→decrypt, AEAD tag tamper fails, IDOR negative (wrong actor → Unauthorized), bearer (no owner) succeeds
- `pointer::tests::end_to_end_offline_1024` — store_offline→embed(1024 DWT, 48 B)→extract→resolve_offline, IDOR negative, 512 offline correctly rejects with `offline pointer requires image at least 1024x1024`
- `pointer::tests::{aead_roundtrip, payload_offline_roundtrip, offline_requires_1024, embed_extract_*}` — unit coverage
- Existing `framed.rs`, `registration.rs`, `integration.rs` still pass (shared stack)

## 11. Verification gates

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo check --lib --target wasm32-unknown-unknown  # capglyph-core + capglyph lib (pointer gated, no clap/glob in wasm)
cargo tree --target wasm32-unknown-unknown | grep -E 'clap|glob|trustmark|c2pa' # expect no output
```

All gates pass (2026-08-31).

## 12. References

- `capglyph-docs/research/media-credential/technology/pointer-and-stego.md` — normative three-profile design, sync/placement ladder
- `capglyph-docs/research/media-credential/technology/capacity-robustness-and-threats.md` — carrier ceilings vs robust/stealth, BCH ladder
- `capglyph-docs/research/media-credential/usage/credential-design.md` — DB schema, atomic consume (pointer reuses same `capability_hash` pattern as `token_hash`)
- `capglyph-core/src/framing.rs` / `ecc.rs` — CBOR + HMAC, BCH/RS + soft LLR
