use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::error::{Result, ServerError};
use crate::models::{
    sha256, AuditEvent, Cover, Credential, CredentialConsumption, NewAuditEvent, NewCover,
    NewCredential,
};

// ── Schema ────────────────────────────────────────────────────────────────────

pub const SCHEMA_SQL: &str = r#"
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
"#;

// ── Db handle ─────────────────────────────────────────────────────────────────

/// Thin wrapper around a SQLite file. Each operation opens a fresh connection
/// via `connect()` to allow concurrent `consume` transactions without sharing
/// a single `Connection` across threads (rusqlite `Connection` is !Sync).
/// For in-memory tests we keep a shared `Arc<Mutex<Connection>>` behind
/// `Db::new_in_memory_shared()` or just use a temp file.
#[derive(Debug, Clone)]
pub struct Db {
    path: Option<PathBuf>,
    // Only used for in-memory (`:memory:`) shared connections in tests.
    mem_conn: Option<Arc<Mutex<Connection>>>,
}

impl Db {
    /// Open or create a SQLite file at `path`, run migrations.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ServerError::Internal(e.to_string()))?;
            }
        }
        let conn = Connection::open(&path)?;
        Self::init_conn(&conn)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            path: Some(path),
            mem_conn: None,
        })
    }

    /// Create an isolated in-memory DB (no file). Each `Db` instance gets its
    /// own private `:memory:` database; `connect()` returns a new connection to
    /// a *different* `:memory:` instance unless we use shared cache. For tests
    /// that need cross-thread sharing, use `new_in_memory_shared()`.
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_conn(&conn)?;
        conn.execute_batch(SCHEMA_SQL)?;
        // For simple single-connection tests we keep the connection for reuse
        // via `path=None` + `mem_conn`. `connect()` will reuse this shared
        // connection (see below).
        Ok(Self {
            path: None,
            mem_conn: Some(Arc::new(Mutex::new(conn))),
        })
    }

    /// Shared in-memory DB that can be cloned across threads (uses a single
    /// `Arc<Mutex<Connection>>` for all operations). Slower but correct for
    /// concurrency tests with `:memory:`.
    pub fn new_in_memory_shared() -> Result<Self> {
        Self::new_in_memory()
    }

    /// Open a temp file-backed DB (recommended for concurrent tests).
    pub fn new_temp_file() -> Result<(Self, tempfile::TempDir)> {
        let dir = tempfile::tempdir().map_err(|e| ServerError::Internal(e.to_string()))?;
        let path = dir.path().join("capglyph.db");
        let db = Self::new(&path)?;
        // Keep dir alive via caller
        Ok((db, dir))
    }

    fn init_conn(conn: &Connection) -> Result<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Busy timeout to handle WAL contention during concurrent consume
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        Ok(())
    }

    fn connect(&self) -> Result<Connection> {
        if let Some(_mem) = &self.mem_conn {
            // For in-memory shared case we clone the underlying connection via
            // backup? Instead we just use the shared mutex directly for all ops.
            // To keep the API uniform, we return a new in-memory connection that
            // shares the same underlying DB via `sqlite://` shared cache URI.
            // Simpler: we will branch in each method to use the shared mutex.
            // Here we return an error if someone tries to use `connect()` with
            // shared memory — callers must go through `with_conn`.
            // Fallback: open a new in-memory connection and re-apply schema
            // (won't share data). So we forbid this path.
            // Instead, expose `with_conn` that handles both.
            // For now, if mem_conn is Some, we create a new connection to the
            // same shared memory using URI `file:memdb1?mode=memory&cache=shared`
            // — we use a static URI with shared cache.
            let conn = Connection::open("file:capglyph_memdb?mode=memory&cache=shared")?;
            Self::init_conn(&conn)?;
            // Ensure schema exists (idempotent)
            conn.execute_batch(SCHEMA_SQL)?;
            Ok(conn)
        } else if let Some(path) = &self.path {
            let conn = Connection::open(path)?;
            Self::init_conn(&conn)?;
            Ok(conn)
        } else {
            Err(ServerError::Internal(
                "Db has no path and no shared mem".into(),
            ))
        }
    }

    /// Helper to run a closure with a connection, handling the shared-memory
    /// mutex case transparently.
    fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        if let Some(mem) = &self.mem_conn {
            let guard = mem
                .lock()
                .map_err(|e| ServerError::Internal(e.to_string()))?;
            f(&guard)
        } else {
            let conn = self.connect()?;
            f(&conn)
        }
    }

    fn with_conn_mut<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T>,
    {
        if let Some(mem) = &self.mem_conn {
            let mut guard = mem
                .lock()
                .map_err(|e| ServerError::Internal(e.to_string()))?;
            f(&mut guard)
        } else {
            let mut conn = self.connect()?;
            f(&mut conn)
        }
    }

    // ── Covers ────────────────────────────────────────────────────────────────

    pub fn create_cover(&self, nc: NewCover) -> Result<Cover> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO covers (id, sha256, object_uri, width, height, format, family_id, issuance_count, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
                params![
                    id.to_string(),
                    nc.sha256,
                    nc.object_uri,
                    nc.width,
                    nc.height,
                    nc.format,
                    nc.family_id.map(|u| u.to_string()),
                    nc.status,
                    now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                ],
            )?;
            Ok(())
        })?;
        self.get_cover(&id)?
            .ok_or_else(|| ServerError::Internal("cover insert failed".into()))
    }

    pub fn get_cover(&self, id: &Uuid) -> Result<Option<Cover>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, sha256, object_uri, width, height, format, family_id, issuance_count, status, created_at FROM covers WHERE id = ?1",
            )?;
            let row = stmt
                .query_row(params![id.to_string()], |r| {
                    Ok(Cover {
                        id: Uuid::from_str(&r.get::<_, String>(0)?).unwrap(),
                        sha256: r.get(1)?,
                        object_uri: r.get(2)?,
                        width: r.get(3)?,
                        height: r.get(4)?,
                        format: r.get(5)?,
                        family_id: r
                            .get::<_, Option<String>>(6)?
                            .map(|s| Uuid::from_str(&s).unwrap()),
                        issuance_count: r.get(7)?,
                        status: r.get(8)?,
                        created_at: r
                            .get::<_, String>(9)?
                            .parse::<DateTime<Utc>>()
                            .unwrap(),
                    })
                })
                .optional()?;
            Ok(row)
        })
    }

    pub fn increment_cover_issuance(&self, cover_id: &Uuid) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE covers SET issuance_count = issuance_count + 1 WHERE id = ?1",
                params![cover_id.to_string()],
            )?;
            Ok(())
        })
    }

    // ── Credentials ───────────────────────────────────────────────────────────

    pub fn create_credential(&self, nc: NewCredential) -> Result<Credential> {
        let id = Uuid::new_v4();
        let token_hash = sha256(&nc.token_id);
        let now = Utc::now();
        let scope_str = serde_json::to_string(&nc.scope).unwrap();
        let embed_params_str = serde_json::to_string(&nc.embed_params).unwrap();
        // output_sha256 is provided or derived from token + cover? For MVP, caller supplies.
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO credentials (id, token_hash, cover_id, subject_id, scope, mode, schema_version, key_id, embed_params, output_sha256, not_before, expires_at, max_uses, use_count, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, ?14)",
                params![
                    id.to_string(),
                    token_hash,
                    nc.cover_id.to_string(),
                    nc.subject_id.map(|u| u.to_string()),
                    scope_str,
                    nc.mode,
                    nc.schema_version,
                    nc.key_id,
                    embed_params_str,
                    nc.output_sha256,
                    nc.not_before.map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
                    nc.expires_at.map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
                    nc.max_uses,
                    now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                ],
            )?;
            Ok(())
        })?;
        // Increment cover issuance count
        let _ = self.increment_cover_issuance(&nc.cover_id);
        // Audit
        let _ = self.insert_audit(NewAuditEvent {
            event_type: "credential.issued".into(),
            object_id: Some(id),
            actor_id: nc.subject_id,
            event_data: Some(serde_json::json!({
                "cover_id": nc.cover_id.to_string(),
                "key_id": nc.key_id,
                "scope": nc.scope,
            })),
        });
        self.get_credential(&id)?
            .ok_or_else(|| ServerError::Internal("credential insert failed".into()))
    }

    pub fn get_credential(&self, id: &Uuid) -> Result<Option<Credential>> {
        self.with_conn(|conn| Self::get_credential_inner(conn, id))
    }

    fn get_credential_inner(conn: &Connection, id: &Uuid) -> Result<Option<Credential>> {
        let mut stmt = conn.prepare(
            "SELECT id, token_hash, cover_id, subject_id, scope, mode, schema_version, key_id, embed_params, output_sha256, not_before, expires_at, max_uses, use_count, revoked_at, created_at
             FROM credentials WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(params![id.to_string()], |r| {
                Ok(Credential {
                    id: Uuid::from_str(&r.get::<_, String>(0)?).unwrap(),
                    token_hash: r.get(1)?,
                    cover_id: Uuid::from_str(&r.get::<_, String>(2)?).unwrap(),
                    subject_id: r
                        .get::<_, Option<String>>(3)?
                        .map(|s| Uuid::from_str(&s).unwrap()),
                    scope: serde_json::from_str(&r.get::<_, String>(4)?).unwrap(),
                    mode: r.get(5)?,
                    schema_version: r.get(6)?,
                    key_id: r.get(7)?,
                    embed_params: serde_json::from_str(&r.get::<_, String>(8)?).unwrap(),
                    output_sha256: r.get(9)?,
                    not_before: r
                        .get::<_, Option<String>>(10)?
                        .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                    expires_at: r
                        .get::<_, Option<String>>(11)?
                        .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                    max_uses: r.get(12)?,
                    use_count: r.get(13)?,
                    revoked_at: r
                        .get::<_, Option<String>>(14)?
                        .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                    created_at: r.get::<_, String>(15)?.parse::<DateTime<Utc>>().unwrap(),
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn get_credential_by_token_hash(&self, token_hash: &[u8]) -> Result<Option<Credential>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, token_hash, cover_id, subject_id, scope, mode, schema_version, key_id, embed_params, output_sha256, not_before, expires_at, max_uses, use_count, revoked_at, created_at
                 FROM credentials WHERE token_hash = ?1",
            )?;
            let row = stmt
                .query_row(params![token_hash], |r| {
                    Ok(Credential {
                        id: Uuid::from_str(&r.get::<_, String>(0)?).unwrap(),
                        token_hash: r.get(1)?,
                        cover_id: Uuid::from_str(&r.get::<_, String>(2)?).unwrap(),
                        subject_id: r
                            .get::<_, Option<String>>(3)?
                            .map(|s| Uuid::from_str(&s).unwrap()),
                        scope: serde_json::from_str(&r.get::<_, String>(4)?).unwrap(),
                        mode: r.get(5)?,
                        schema_version: r.get(6)?,
                        key_id: r.get(7)?,
                        embed_params: serde_json::from_str(&r.get::<_, String>(8)?).unwrap(),
                        output_sha256: r.get(9)?,
                        not_before: r
                            .get::<_, Option<String>>(10)?
                            .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                        expires_at: r
                            .get::<_, Option<String>>(11)?
                            .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                        max_uses: r.get(12)?,
                        use_count: r.get(13)?,
                        revoked_at: r
                            .get::<_, Option<String>>(14)?
                            .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                        created_at: r.get::<_, String>(15)?.parse::<DateTime<Utc>>().unwrap(),
                    })
                })
                .optional()?;
            Ok(row)
        })
    }

    // ── Verify (read-only) ──────────────────────────────────────────────────

    pub fn verify_by_token_hash(&self, token_hash: &[u8]) -> Result<Credential> {
        let cred = self
            .get_credential_by_token_hash(token_hash)?
            .ok_or_else(|| ServerError::NotFound("credential not found".into()))?;
        Self::check_credential_state(&cred)?;
        Ok(cred)
    }

    fn check_credential_state(cred: &Credential) -> Result<()> {
        if cred.revoked_at.is_some() {
            return Err(ServerError::Revoked);
        }
        let now = Utc::now();
        if let Some(nb) = cred.not_before {
            if now < nb {
                return Err(ServerError::Expired);
            }
        }
        if let Some(ea) = cred.expires_at {
            if now >= ea {
                return Err(ServerError::Expired);
            }
        }
        if let Some(max) = cred.max_uses {
            if cred.use_count >= max {
                return Err(ServerError::Exhausted);
            }
        }
        Ok(())
    }

    // ── Atomic consume ──────────────────────────────────────────────────────

    /// Atomic consume with idempotency. Must be transactional.
    /// Returns the updated credential on success.
    /// Idempotent replay: if the same `idempotency_key` was already used for
    /// this credential, return the previous result without incrementing.
    pub fn consume(
        &self,
        token_hash: &[u8],
        idempotency_key: &str,
        actor_id: Option<Uuid>,
        request_hash: Option<Vec<u8>>,
    ) -> Result<Credential> {
        self.with_conn_mut(|conn| {
            // Use IMMEDIATE to acquire reserved lock early and avoid deadlock busy loops
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            let result: Result<Credential> = (|| {
                // 1. Find credential by token_hash
                let cred_opt = {
                    let mut stmt = conn.prepare(
                        "SELECT id, token_hash, cover_id, subject_id, scope, mode, schema_version, key_id, embed_params, output_sha256, not_before, expires_at, max_uses, use_count, revoked_at, created_at
                         FROM credentials WHERE token_hash = ?1",
                    )?;
                    stmt.query_row(params![token_hash], |r| {
                        Ok(Credential {
                            id: Uuid::from_str(&r.get::<_, String>(0)?).unwrap(),
                            token_hash: r.get(1)?,
                            cover_id: Uuid::from_str(&r.get::<_, String>(2)?).unwrap(),
                            subject_id: r
                                .get::<_, Option<String>>(3)?
                                .map(|s| Uuid::from_str(&s).unwrap()),
                            scope: serde_json::from_str(&r.get::<_, String>(4)?).unwrap(),
                            mode: r.get(5)?,
                            schema_version: r.get(6)?,
                            key_id: r.get(7)?,
                            embed_params: serde_json::from_str(&r.get::<_, String>(8)?).unwrap(),
                            output_sha256: r.get(9)?,
                            not_before: r
                                .get::<_, Option<String>>(10)?
                                .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                            expires_at: r
                                .get::<_, Option<String>>(11)?
                                .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                            max_uses: r.get(12)?,
                            use_count: r.get(13)?,
                            revoked_at: r
                                .get::<_, Option<String>>(14)?
                                .map(|s| s.parse::<DateTime<Utc>>().unwrap()),
                            created_at: r.get::<_, String>(15)?.parse::<DateTime<Utc>>().unwrap(),
                        })
                    })
                    .optional()?
                };
                let cred = cred_opt.ok_or_else(|| ServerError::NotFound("credential not found".into()))?;

                // 2. Check idempotency: has this key already been used for this credential?
                let existing: Option<(String, String)> = {
                    let mut stmt = conn.prepare(
                        "SELECT id, outcome FROM credential_consumptions WHERE credential_id = ?1 AND idempotency_key = ?2",
                    )?;
                    stmt.query_row(
                        params![cred.id.to_string(), idempotency_key],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                    )
                    .optional()?
                };
                if let Some((_id, _outcome)) = existing {
                    // Idempotent replay — return current credential without mutating.
                    // Outcome must have been success previously; we treat replay as success.
                    // Verify credential still looks valid (but allow replay even if now expired? spec says idempotent).
                    // For strictness, we return credential as-is.
                    // Need to ensure we didn't already count this consumption; replay should not increment.
                    // Just return the credential with current use_count.
                    // The caller can distinguish replay via `outcome`.
                    // We do not insert audit again.
                    return Ok(cred);
                }

                // 3. Check credential state before consuming (fail-closed)
                // Use the same logic as verify, but inline to avoid borrowing issues.
                if cred.revoked_at.is_some() {
                    // Record failed consumption for audit
                    let _ = Self::insert_consumption_and_audit(
                        conn,
                        &cred.id,
                        idempotency_key,
                        actor_id,
                        request_hash.as_deref(),
                        "revoked",
                    );
                    return Err(ServerError::Revoked);
                }
                let now = Utc::now();
                if let Some(nb) = cred.not_before {
                    if now < nb {
                        let _ = Self::insert_consumption_and_audit(
                            conn,
                            &cred.id,
                            idempotency_key,
                            actor_id,
                            request_hash.as_deref(),
                            "not_yet_valid",
                        );
                        return Err(ServerError::Expired);
                    }
                }
                if let Some(ea) = cred.expires_at {
                    if now >= ea {
                        let _ = Self::insert_consumption_and_audit(
                            conn,
                            &cred.id,
                            idempotency_key,
                            actor_id,
                            request_hash.as_deref(),
                            "expired",
                        );
                        return Err(ServerError::Expired);
                    }
                }
                if let Some(max) = cred.max_uses {
                    if cred.use_count >= max {
                        let _ = Self::insert_consumption_and_audit(
                            conn,
                            &cred.id,
                            idempotency_key,
                            actor_id,
                            request_hash.as_deref(),
                            "exhausted",
                        );
                        return Err(ServerError::Exhausted);
                    }
                }

                // 4. Atomic UPDATE ... RETURNING
                // SQLite supports RETURNING since 3.35. We use it to ensure only one
                // writer increments when multiple threads race. The WHERE clause
                // includes the same checks as above to make it safe under concurrency.
                let mut stmt = conn.prepare(
                    "UPDATE credentials
                     SET use_count = use_count + 1
                     WHERE id = ?1
                       AND revoked_at IS NULL
                       AND (not_before IS NULL OR not_before <= ?2)
                       AND (expires_at IS NULL OR expires_at > ?2)
                       AND (max_uses IS NULL OR use_count < max_uses)
                     RETURNING use_count",
                )?;
                let now_str = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let new_use_count: Option<i64> = stmt
                    .query_row(params![cred.id.to_string(), now_str], |r| r.get(0))
                    .optional()?;
                let new_use_count = match new_use_count {
                    Some(v) => v,
                    None => {
                        // Another concurrent transaction raced us and exhausted quota
                        let _ = Self::insert_consumption_and_audit(
                            conn,
                            &cred.id,
                            idempotency_key,
                            actor_id,
                            request_hash.as_deref(),
                            "exhausted_race",
                        );
                        return Err(ServerError::Exhausted);
                    }
                };

                // 5. Insert consumption record (unique per idempotency_key)
                Self::insert_consumption_and_audit(
                    conn,
                    &cred.id,
                    idempotency_key,
                    actor_id,
                    request_hash.as_deref(),
                    "consumed",
                )?;

                // 6. Return updated credential
                let mut updated = cred;
                updated.use_count = new_use_count;
                Ok(updated)
            })();

            match &result {
                Ok(_) => {
                    conn.execute_batch("COMMIT;")?;
                }
                Err(_) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                }
            }
            result
        })
    }

    fn insert_consumption_and_audit(
        conn: &Connection,
        credential_id: &Uuid,
        idempotency_key: &str,
        actor_id: Option<Uuid>,
        request_hash: Option<&[u8]>,
        outcome: &str,
    ) -> Result<()> {
        let cid = Uuid::new_v4();
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        conn.execute(
            "INSERT INTO credential_consumptions (id, credential_id, idempotency_key, actor_id, consumed_at, request_hash, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                cid.to_string(),
                credential_id.to_string(),
                idempotency_key,
                actor_id.map(|u| u.to_string()),
                now,
                request_hash,
                outcome,
            ],
        )?;
        // Audit event for consumption
        let aid = Uuid::new_v4();
        let event_data = serde_json::json!({
            "credential_id": credential_id.to_string(),
            "idempotency_key": idempotency_key,
            "outcome": outcome,
            "actor_id": actor_id.map(|u| u.to_string()),
        });
        conn.execute(
            "INSERT INTO audit_events (id, event_type, object_id, actor_id, event_data, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                aid.to_string(),
                format!("credential.{}", outcome),
                credential_id.to_string(),
                actor_id.map(|u| u.to_string()),
                serde_json::to_string(&event_data).unwrap(),
                now,
            ],
        )?;
        Ok(())
    }

    // ── Revoke ────────────────────────────────────────────────────────────────

    pub fn revoke(&self, credential_id: &Uuid, actor_id: Option<Uuid>) -> Result<Credential> {
        self.with_conn_mut(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            let result: Result<Credential> = (|| {
                let cred_opt = Self::get_credential_inner(conn, credential_id)?;
                let cred = cred_opt.ok_or_else(|| ServerError::NotFound("credential not found".into()))?;
                if cred.revoked_at.is_some() {
                    return Err(ServerError::Conflict("already revoked".into()));
                }
                let now = Utc::now();
                let now_str = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                conn.execute(
                    "UPDATE credentials SET revoked_at = ?1 WHERE id = ?2",
                    params![now_str, credential_id.to_string()],
                )?;
                // Audit
                let aid = Uuid::new_v4();
                conn.execute(
                    "INSERT INTO audit_events (id, event_type, object_id, actor_id, event_data, occurred_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        aid.to_string(),
                        "credential.revoked",
                        credential_id.to_string(),
                        actor_id.map(|u| u.to_string()),
                        serde_json::to_string(&serde_json::json!({"revoked_at": now_str})).unwrap(),
                        now_str,
                    ],
                )?;
                let mut updated = cred;
                updated.revoked_at = Some(now);
                Ok(updated)
            })();
            match &result {
                Ok(_) => {
                    conn.execute_batch("COMMIT;")?;
                }
                Err(_) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                }
            }
            result
        })
    }

    // ── Audit ─────────────────────────────────────────────────────────────────

    pub fn insert_audit(&self, ev: NewAuditEvent) -> Result<AuditEvent> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let ev_type = ev.event_type.clone();
        let ev_obj = ev.object_id;
        let ev_actor = ev.actor_id;
        let ev_data_str = ev
            .event_data
            .clone()
            .map(|v| serde_json::to_string(&v).unwrap());
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO audit_events (id, event_type, object_id, actor_id, event_data, occurred_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id.to_string(),
                    ev_type,
                    ev_obj.map(|u| u.to_string()),
                    ev_actor.map(|u| u.to_string()),
                    ev_data_str,
                    now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                ],
            )?;
            Ok(())
        })?;
        Ok(AuditEvent {
            id,
            event_type: ev.event_type,
            object_id: ev.object_id,
            actor_id: ev.actor_id,
            event_data: ev.event_data,
            occurred_at: now,
        })
    }

    pub fn list_audit_events(
        &self,
        object_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<AuditEvent>> {
        self.with_conn(|conn| {
            let mut events = Vec::new();
            if let Some(oid) = object_id {
                let mut stmt = conn.prepare(
                    "SELECT id, event_type, object_id, actor_id, event_data, occurred_at FROM audit_events WHERE object_id = ?1 ORDER BY occurred_at DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![oid.to_string(), limit], |r| {
                    Ok(AuditEvent {
                        id: Uuid::from_str(&r.get::<_, String>(0)?).unwrap(),
                        event_type: r.get(1)?,
                        object_id: r
                            .get::<_, Option<String>>(2)?
                            .map(|s| Uuid::from_str(&s).unwrap()),
                        actor_id: r
                            .get::<_, Option<String>>(3)?
                            .map(|s| Uuid::from_str(&s).unwrap()),
                        event_data: r
                            .get::<_, Option<String>>(4)?
                            .map(|s| serde_json::from_str(&s).unwrap()),
                        occurred_at: r.get::<_, String>(5)?.parse::<DateTime<Utc>>().unwrap(),
                    })
                })?;
                for row in rows {
                    events.push(row?);
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, event_type, object_id, actor_id, event_data, occurred_at FROM audit_events ORDER BY occurred_at DESC LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit], |r| {
                    Ok(AuditEvent {
                        id: Uuid::from_str(&r.get::<_, String>(0)?).unwrap(),
                        event_type: r.get(1)?,
                        object_id: r
                            .get::<_, Option<String>>(2)?
                            .map(|s| Uuid::from_str(&s).unwrap()),
                        actor_id: r
                            .get::<_, Option<String>>(3)?
                            .map(|s| Uuid::from_str(&s).unwrap()),
                        event_data: r
                            .get::<_, Option<String>>(4)?
                            .map(|s| serde_json::from_str(&s).unwrap()),
                        occurred_at: r.get::<_, String>(5)?.parse::<DateTime<Utc>>().unwrap(),
                    })
                })?;
                for row in rows {
                    events.push(row?);
                }
            }
            Ok(events)
        })
    }

    pub fn list_consumptions(&self, credential_id: &Uuid) -> Result<Vec<CredentialConsumption>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, credential_id, idempotency_key, actor_id, consumed_at, request_hash, outcome FROM credential_consumptions WHERE credential_id = ?1 ORDER BY consumed_at ASC",
            )?;
            let rows = stmt.query_map(params![credential_id.to_string()], |r| {
                Ok(CredentialConsumption {
                    id: Uuid::from_str(&r.get::<_, String>(0)?).unwrap(),
                    credential_id: Uuid::from_str(&r.get::<_, String>(1)?).unwrap(),
                    idempotency_key: r.get(2)?,
                    actor_id: r
                        .get::<_, Option<String>>(3)?
                        .map(|s| Uuid::from_str(&s).unwrap()),
                    consumed_at: r.get::<_, String>(4)?.parse::<DateTime<Utc>>().unwrap(),
                    request_hash: r.get(5)?,
                    outcome: r.get(6)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }
}
