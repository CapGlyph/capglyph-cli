-- 002_pointer.sql — CTX-0024 pointer mode: message_objects (capability → encrypted object)
-- Extends 001_initial with pointer stego tables.

CREATE TABLE IF NOT EXISTS message_objects (
    id              TEXT PRIMARY KEY,
    capability_id   BLOB NOT NULL UNIQUE,
    capability_hash BLOB NOT NULL UNIQUE,
    ciphertext      BLOB NOT NULL,
    nonce           BLOB NOT NULL,
    tag             BLOB NOT NULL,
    content_key     BLOB,
    policy          TEXT NOT NULL,
    owner_id        TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    expires_at      TEXT
);
CREATE INDEX IF NOT EXISTS idx_message_objects_cap_hash ON message_objects(capability_hash);
CREATE INDEX IF NOT EXISTS idx_message_objects_owner ON message_objects(owner_id);
