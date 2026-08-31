//! capglyph-server (sigild) — credential vault MVP
//!
//! Implements `docs/research/media-credential/usage/credential-design.md` §4:
//! `covers / credentials / credential_consumptions / audit_events` with
//! `UPDATE ... RETURNING` atomic consume, idempotency, revocation and audit.
//!
//! Carrier integration via `capglyph_core::{framing,ecc,carrier}`.

pub mod carrier_integration;
pub mod db;
pub mod error;
pub mod models;
pub mod service;

pub mod http;

pub use db::Db;
pub use error::{Result, ServerError};
pub use service::{Kms, Service};

pub use http::router;
