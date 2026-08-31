-- 001_initial.sql — sigild MVP schema per docs/research/media-credential/usage/credential-design.md §4
-- Covers / credentials / credential_consumptions / audit_events
-- Postgres normative; SQLite-compatible (TEXT for UUID, BLOB for BYTEA, TEXT for JSONB/TIMESTAMPTZ)

PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS covers (
    id              TEXT PRIMARY KEY,
    sha256          BLOB NOT NULL UNIQUE,
    object_uri      TEXT NOT NULL,
    width           INTEGER NOT NULL,
    height          INTEGER NOT NULL,
    format          TEXT NOT NULL,
    family_id       TEXT,
    issuance_count  INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE IF NOT EXISTS credentials (
    id              TEXT PRIMARY KEY,
    token_hash      BLOB NOT NULL UNIQUE,
    cover_id        TEXT NOT NULL REFERENCES covers(id),
    subject_id      TEXT,
    scope           TEXT NOT NULL,
    mode            TEXT NOT NULL,
    schema_version  INTEGER NOT NULL,
    key_id          TEXT NOT NULL,
    embed_params    TEXT NOT NULL,
    output_sha256   BLOB NOT NULL,
    not_before      TEXT,
    expires_at      TEXT,
    max_uses        INTEGER,
    use_count       INTEGER NOT NULL DEFAULT 0,
    revoked_at      TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_credentials_token_hash ON credentials(token_hash);
CREATE INDEX IF NOT EXISTS idx_credentials_cover_id ON credentials(cover_id);

CREATE TABLE IF NOT EXISTS credential_consumptions (
    id              TEXT PRIMARY KEY,
    credential_id   TEXT NOT NULL REFERENCES credentials(id),
    idempotency_key TEXT NOT NULL,
    actor_id        TEXT,
    consumed_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    request_hash    BLOB,
    outcome         TEXT NOT NULL,
    UNIQUE (credential_id, idempotency_key)
);
CREATE INDEX IF NOT EXISTS idx_consumptions_credential ON credential_consumptions(credential_id);

CREATE TABLE IF NOT EXISTS audit_events (
    id              TEXT PRIMARY KEY,
    event_type      TEXT NOT NULL,
    object_id       TEXT,
    actor_id        TEXT,
    event_data      TEXT,
    occurred_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_audit_object ON audit_events(object_id);
CREATE INDEX IF NOT EXISTS idx_audit_type ON audit_events(event_type);
