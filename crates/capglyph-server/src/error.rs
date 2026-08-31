/// Central error type for capglyph-server (sigild).

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found: {0}")]
    NotFound(String),
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
    #[error("unauthorized scope: {0}")]
    Unauthorized(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, ServerError>;

impl From<anyhow::Error> for ServerError {
    fn from(e: anyhow::Error) -> Self {
        ServerError::Internal(e.to_string())
    }
}
