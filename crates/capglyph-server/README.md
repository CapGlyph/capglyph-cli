# capglyph-server (sigild) — Credential Vault MVP

Implements `docs/research/media-credential/usage/credential-design.md` §4 (`covers`, `credentials`, `credential_consumptions`, `audit_events`) with:

- **SQLite** (rusqlite, `bundled`) — Postgres-compatible schema (`TEXT` UUID, `BLOB` BYTEA, `TEXT` JSONB/TIMESTAMPTZ), `WAL` + `busy_timeout 5s`, `foreign_keys ON`.
- **Atomic consume** via `UPDATE ... RETURNING` inside `BEGIN IMMEDIATE`:

```sql
UPDATE credentials
SET use_count = use_count + 1
WHERE id = $1
  AND revoked_at IS NULL
  AND (not_before IS NULL OR not_before <= now())
  AND (expires_at IS NULL OR expires_at > now())
  AND (max_uses IS NULL OR use_count < max_uses)
RETURNING use_count;
```

Only a returned row authorizes. Must be transactional with `credential_consumptions` insertion via caller-supplied `Idempotency-Key` so network retries don't burn quota twice. Separate `POST /v1/credentials/verify` (read-only) from `POST /v1/credentials/consume` (mutating).

- **Idempotency**: `UNIQUE (credential_id, idempotency_key)`. Replay with same key returns current `use_count` without incrementing (see `tests/concurrent_consume.rs`).
- **Revocation / audit**: `revoked_at` + `audit_events` (`credential.issued`, `credential.consumed`, `credential.revoked`, `credential.*` failure). `GET /v1/credentials/{id}` and `POST /v1/credentials/{id}/revoke`.
- **Carrier integration**: `capglyph_core::{framing,ecc}` — `encode_credential_token` (`token_id → framing::seal(CBOR) → ecc::encode(Repetition8)`) and `decode_credential_token` with soft-bit `LLR` path (see `src/carrier_integration.rs`). KMS split `K_mac`/`K_embed` via `HMAC-SHA256(master, domain || cover_id || token_id)` (§4.4 `KeyMaterial` simplified).
- **HTTP**: `axum` router at `src/http.rs`:

| Endpoint                     | Method | Effect                                                              |
| ---------------------------- | ------ | ------------------------------------------------------------------- |
| `/v1/credentials`            | POST   | issue (generates 128-bit token, returns `token_id` base64url once)  |
| `/v1/credentials/verify`     | POST   | verify (read-only)                                                  |
| `/v1/credentials/consume`    | POST   | atomic consume (`Idempotency-Key` header or `idempotency_key` body) |
| `/v1/credentials/:id`        | GET    | status                                                              |
| `/v1/credentials/:id/revoke` | POST   | revoke                                                              |

Binary `capglyphd` (`src/bin/capglyphd.rs`): `capglyphd --db /tmp/capglyphd.db --listen 127.0.0.1:3000` (env `CAPGLYPHD_MASTER_KEY` hex32 for persistence, else ephemeral).

## Running

```bash
cargo run -p capglyph-server --bin capglyphd -- --db /tmp/capglyphd.db --listen 127.0.0.1:3000
# issue
curl -X POST http://127.0.0.1:3000/v1/credentials -H 'content-type: application/json' \
  -d '{"cover_id":"<uuid>","scope":["download:asset:42"],"max_uses":1}'
# verify (read-only, no burn)
curl -X POST http://127.0.0.1:3000/v1/credentials/verify -H 'content-type: application/json' \
  -d '{"token_id":"<base64url>"}'
# consume (atomic, idempotent)
curl -X POST http://127.0.0.1:3000/v1/credentials/consume -H 'content-type: application/json' -H 'Idempotency-Key: idem-1' \
  -d '{"token_id":"<base64url>","idempotency_key":"idem-1"}'
```

## Tests

- `cargo test -p capglyph-server --test concurrent_consume` — **no double-spend**: 10 threads vs `max_uses=1` → exactly 1 success; `max_uses=3` → exactly 3 successes; idempotent replay doesn't double-count; `verify` is read-only; `revoked`/`expired` are fail-closed; audit trail; `framing+ecc` round-trip.
- `cargo test -p capglyph-server` — unit tests for `carrier_integration` + `http` (issue→verify→consume→revoke flow).

WASM: `cargo check --lib --target wasm32-unknown-unknown --no-default-features -p capglyph` — `capglyph-server` is not in the wasm graph (separate crate, not a `capglyph` lib dependency).
