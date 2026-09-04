//! Central error type for capglyph-server (sigild).

pub use capglyph_core::error::{CapGlyphError, ErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("key not found: {0}")]
    KeyNotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("expired")]
    Expired,
    #[error("revoked")]
    Revoked,
    #[error("exhausted: quota reached")]
    Exhausted,
    #[error("invalid token")]
    InvalidToken,
    #[error("scope denied")]
    ScopeDenied,
    #[error("unauthorized scope: {0}")]
    Unauthorized(String),
    #[error(transparent)]
    Core(#[from] CapGlyphError),
    #[error("internal: {0}")]
    Internal(String),
}

impl ServerError {
    /// Stable machine-readable code returned on the HTTP wire.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "E_NOT_FOUND",
            Self::KeyNotFound(_) => ErrorCode::KeyNotFound.as_str(),
            Self::Conflict(_) => "E_CONFLICT",
            Self::Expired => ErrorCode::Expired.as_str(),
            Self::Revoked => ErrorCode::Revoked.as_str(),
            Self::Exhausted => ErrorCode::Consumed.as_str(),
            Self::InvalidToken => ErrorCode::PayloadInvalid.as_str(),
            Self::ScopeDenied => "E_SCOPE_DENIED",
            Self::Unauthorized(_) => "E_UNAUTHORIZED",
            Self::Core(error) => error.code.as_str(),
            Self::Db(_) | Self::Internal(_) => ErrorCode::Internal.as_str(),
        }
    }

    /// Stable error name without the `E_` prefix.
    pub const fn error_name(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "NOT_FOUND",
            Self::KeyNotFound(_) => "KEY_NOT_FOUND",
            Self::Conflict(_) => "CONFLICT",
            Self::Expired => "EXPIRED",
            Self::Revoked => "REVOKED",
            Self::Exhausted => "CONSUMED",
            Self::InvalidToken => "PAYLOAD_INVALID",
            Self::ScopeDenied => "SCOPE_DENIED",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::Core(error) => match error.code {
                ErrorCode::VersionUnsupported => "VERSION_UNSUPPORTED",
                ErrorCode::KeyNotFound => "KEY_NOT_FOUND",
                ErrorCode::MalformedFrame => "MALFORMED_FRAME",
                ErrorCode::AuthFailed => "AUTH_FAILED",
                ErrorCode::PayloadInvalid => "PAYLOAD_INVALID",
                ErrorCode::InsufficientCapacity => "INSUFFICIENT_CAPACITY",
                ErrorCode::Expired => "EXPIRED",
                ErrorCode::Revoked => "REVOKED",
                ErrorCode::Consumed => "CONSUMED",
                ErrorCode::Tampered => "TAMPERED",
                ErrorCode::GeometryMismatch => "GEOMETRY_MISMATCH",
                ErrorCode::Internal => "INTERNAL",
            },
            Self::Db(_) | Self::Internal(_) => "INTERNAL",
        }
    }

    /// Public message deliberately omits database, key, token, and policy details.
    pub fn public_message(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "resource not found",
            Self::KeyNotFound(_) => "key not found",
            Self::Conflict(_) => "request conflicts with existing state",
            Self::Expired => "credential is outside its validity window",
            Self::Revoked => "credential is revoked",
            Self::Exhausted => "credential quota is exhausted",
            Self::InvalidToken => "credential token is invalid",
            Self::ScopeDenied => "requested scope is not authorized",
            Self::Unauthorized(_) => "caller is not authorized",
            Self::Core(error) => match error.code {
                ErrorCode::VersionUnsupported => "CapGlyph version is not supported",
                ErrorCode::KeyNotFound => "key not found",
                ErrorCode::MalformedFrame => "credential frame is malformed",
                ErrorCode::AuthFailed => "credential authentication failed",
                ErrorCode::PayloadInvalid => "credential payload is invalid",
                ErrorCode::InsufficientCapacity => "carrier capacity is insufficient",
                ErrorCode::Expired => "credential is outside its validity window",
                ErrorCode::Revoked => "credential is revoked",
                ErrorCode::Consumed => "credential quota is exhausted",
                ErrorCode::Tampered => "carrier evidence is tampered",
                ErrorCode::GeometryMismatch => "image geometry does not match",
                ErrorCode::Internal => "internal error",
            },
            Self::Db(_) | Self::Internal(_) => "internal error",
        }
    }
}

pub type Result<T> = std::result::Result<T, ServerError>;

impl From<anyhow::Error> for ServerError {
    fn from(e: anyhow::Error) -> Self {
        ServerError::Internal(e.to_string())
    }
}
