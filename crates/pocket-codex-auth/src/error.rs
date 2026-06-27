//! Auth error type.

/// Errors from the auth layer.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// A persistence operation failed.
    #[error("store: {0}")]
    Store(#[from] pocket_codex_store::StoreError),
    /// An HTTP call to GitHub failed.
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    /// JWT encode/decode failed (e.g. invalid or expired token).
    #[error("jwt: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    /// GitHub returned an unexpected error in the device flow.
    #[error("github: {0}")]
    Github(String),
    /// The presented refresh token is unknown, revoked or expired.
    #[error("invalid or expired refresh token")]
    BadRefresh,
}

/// Convenience result alias for the auth layer.
pub type Result<T> = std::result::Result<T, AuthError>;
