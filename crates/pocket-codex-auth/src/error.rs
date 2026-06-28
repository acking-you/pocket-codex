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
    /// The web (authorization-code) flow is not configured on this backend
    /// (no GitHub client secret / public callback URL).
    #[error("web login flow is not configured")]
    WebDisabled,
    /// A web-flow `redirect_uri` was not on the allowlist (custom scheme or
    /// loopback http only).
    #[error("redirect_uri is not allowed")]
    BadRedirect,
    /// The web-flow `state` was unknown, already consumed, or expired (a stale
    /// or forged GitHub callback).
    #[error("invalid or expired web login state")]
    BadWebState,
    /// The one-time exchange code was unknown, already redeemed, expired, or
    /// its PKCE verifier did not match.
    #[error("invalid or expired exchange code")]
    BadExchange,
}

/// Convenience result alias for the auth layer.
pub type Result<T> = std::result::Result<T, AuthError>;
