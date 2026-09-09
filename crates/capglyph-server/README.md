# capglyph-server — Reference Credential and Message Service

**Status:** SQLite/token-oriented reference implementation, not the complete image-credential service. See [source router](src/http.rs), [service](src/service.rs), and [database](src/db.rs). The wider [credential design](../../../capglyph-docs/research/media-credential/usage/credential-design.md) describes target capabilities beyond this implementation.

## Current surface

| Endpoint                      | Method | Role                                                     |
| ----------------------------- | ------ | -------------------------------------------------------- |
| `/v1/version`                 | GET    | API and wire version information                         |
| `/v1/credentials`             | POST   | Issue a token-oriented credential                        |
| `/v1/credentials/verify`      | POST   | Read-only token verification                             |
| `/v1/credentials/consume`     | POST   | Authorized, atomic consume with required idempotency key |
| `/v1/credentials/{id}`        | GET    | Credential status                                        |
| `/v1/credentials/{id}/revoke` | POST   | Authorized revocation                                    |
| `/v1/messages`                | POST   | Store an encrypted message object                        |
| `/v1/messages/resolve`        | POST   | Resolve by capability                                    |

The wider SDK `/v1/seal/open/embed/verify/extract/consume/revoke/info` routes and `GET /health` are **not** this router. No image-based issue/verify route or production cover-vault service is supplied. `CGCLI-0002` (G11 image path) is ready and `CGCLI-0004` (product-gate revalidation) is planned in CarryCtx.

## Keys and startup

For in-memory development, run from the `capglyph-cli` workspace:

```bash
cargo run -p capglyph-server --bin capglyphd -- --listen 127.0.0.1:3000
```

Without `--db`, the process can generate ephemeral development keys. Persistent storage with `--db PATH` **requires** `CAPGLYPHD_MASTER_KEY`: exactly 32 hex-decoded bytes, not the all-zero key. Missing or malformed key material fails startup; there is no persistent-database ephemeral fallback. Set this variable through trusted secret management, not a documented reusable demo secret.

The service derives roles through `KeyMaterial::from_ikm_v1(master, cover_id.as_bytes())` using the [RFC 5869 domain schedule](../../../capglyph-spec/docs/interoperability-profile.md), not the earlier simplified HMAC construction. Unknown key IDs fail closed.

## Authorization boundary

`x-capglyph-actor-id` is trusted ingress context, not authentication by itself. A production ingress must authenticate the actor and strip/replace public actor headers. Body actor claims cannot override ingress identity; subject and scope authorization precede consumption.

Read the request types in [http.rs](src/http.rs) for exact fields and error handling. Demo cover seeding is a development convenience, not an image enrollment pipeline. Do not expose this reference service as production-ready based on a token round trip.

## Atomic consumption

SQLite uses `BEGIN IMMEDIATE` and a guarded `UPDATE ... RETURNING` with revocation, not-before, expiry, and quota predicates. The temporal upper bound is exclusive: `now >= expires_at` is expired. Read-only verify does not burn quota.

Consume requires caller-supplied `Idempotency-Key` or the corresponding request field; conflicting values fail. Stored idempotency records bind the request context, so reuse with different request data is not a valid replay. Transactions coordinate the state update and consumption record. A successful SQL row is only part of the authorized service operation, not permission to bypass subject/scope checks.

SQLite storage types and transaction semantics are not a drop-in Postgres deployment. Postgres, production KMS, persisted cover retrieval, and image-based verification require their own implementation and gates.

## Verification commands

```bash
cargo test -p capglyph-server
cargo test -p capglyph-server --test concurrent_consume
cargo check --lib --target wasm32-unknown-unknown --no-default-features -p capglyph
```

Run from the CLI workspace. These commands describe how to check the implementation; this documentation audit does not claim a new test run. The server is a separate native workspace member, not a dependency of the WASM library graph. Framing/ECC helpers do not establish a complete image transport path.
