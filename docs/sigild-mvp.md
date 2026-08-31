# sigild (capglyphd) MVP — DB + Atomic Consume + Revocation/Audit (CTX-0023)

**Status:** Implemented 2026-08-31
**Crate:** `crates/capglyph-server` (binary `capglyphd`)
**Spec:** `docs/research/media-credential/usage/credential-design.md` §4

## Schema

Migrations: `crates/capglyph-server/migrations/001_initial.sql` — Postgres normative, SQLite-compatible. Tables: `covers`, `credentials`, `credential_consumptions`, `audit_events` (see `SCHEMA_SQL` in `src/db.rs`). Indexes on `token_hash`, `cover_id`, `credential_id`, `audit object/type`. `WAL`, `busy_timeout 5s`.

`token_hash = SHA-256(token_id)` — raw 128-bit token (CSPRNG, base64url outside carrier) never appears in logs; `scope`/`embed_params`/`event_data` are `JSONB` (`TEXT` in SQLite).

## Atomic consume

Spec `UPDATE ... RETURNING` inside `BEGIN IMMEDIATE` (see `src/db.rs::consume`):

- `GET` credential by `token_hash`
- Check `UNIQUE (credential_id, idempotency_key)` → idempotent replay returns current `use_count` without increment
- Fail-closed checks: `revoked_at IS NULL`, `not_before <= now() <= expires_at`, `use_count < max_uses`
- `UPDATE ... RETURNING use_count` — only returned row authorizes; race losers get `Exhausted` (`exhausted_race`)
- `INSERT INTO credential_consumptions` + `audit_events (credential.consumed)` in same `COMMIT`

`verify` is `SELECT` only — must never burn quota.

## Revocation / audit

- `POST /v1/credentials/{id}/revoke` → `UPDATE credentials SET revoked_at = now()` + `audit_events (credential.revoked)`. Any terminal state (`REVOKED`, `EXPIRED`, `CONSUMED`) is fail-closed.
- `GET /v1/credentials/{id}` → current record.
- `audit_events` captures `issued`, `consumed`, `revoked`, `expired`/`exhausted` failures; `list_audit_events` / `list_consumptions` helpers.

## Carrier integration

`src/carrier_integration.rs` demonstrates `capglyph_core::{framing,ecc}`:

- `encode_credential_token(token_id, K_mac)` = `framing::seal([token_id], Params::Credential, K_mac) → ecc::encode(Repetition8)`
- `decode_credential_token(coded, K_mac)` = `ecc::decode_hard → framing::open → token_id` (hard-bit) and `decode_credential_token_soft` (soft-bit `LLR` via `SoftBit::from_coeff`).

Service `issue` does `seal → ecc::encode` (carrier lattice not yet exercised against real image; `output_sha256` is `SHA256(sealed)` placeholder). `verify`/`consume` re-derive `K_mac/K_embed` via `Kms::derive(master, domain, cover_id, token_id)` (`HMAC-SHA256`) and verify framing tag before DB mutation.

## HTTP

`src/http.rs` (`axum`):

- `POST /v1/credentials` (issue)
- `POST /v1/credentials/verify` (read-only)
- `POST /v1/credentials/consume` (mutating, `Idempotency-Key` header)
- `GET /v1/credentials/:id`
- `POST /v1/credentials/:id/revoke`

See `src/http.rs::tests::issue_then_verify_then_consume_then_revoke`.

## Tests

`cargo test -p capglyph-server --test concurrent_consume` proves no double-spend:

- 10 threads vs `max_uses=1` → 1 success
- 10 threads vs `max_uses=3` → 3 successes
- Idempotent replay with same `idempotency_key` doesn't increment
- `verify` doesn't bump `use_count`
- `revoked`/`expired` are fail-closed
- Audit trail + framing round-trip

`cargo test -p capglyph-server` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo check --lib --target wasm32-unknown-unknown --no-default-features -p capglyph` all pass.

## Binary

`cargo run -p capglyph-server --bin capglyphd -- --db /tmp/capglyphd.db --listen 127.0.0.1:3000` (`CAPGLYPHD_MASTER_KEY` hex32 or ephemeral).
