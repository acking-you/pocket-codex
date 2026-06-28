//! Row types mapped from the schema. All timestamps are unix seconds.

use sqlx::FromRow;

/// A hosted-account user, identified internally by [`User::id`] (the relay-key
/// namespace) and externally by their GitHub identity.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct User {
    /// Internal user id; used as the `pcxu:<id>:…` relay-key namespace.
    pub id: String,
    /// GitHub numeric account id.
    pub github_id: i64,
    /// GitHub login/handle (display).
    pub github_login: String,
    /// First-seen time (unix seconds).
    pub created_at: i64,
    /// Most recent login time (unix seconds).
    pub last_login_at: i64,
}

/// A stored refresh token. Only the SHA-256 [`RefreshToken::token_hash`] is
/// persisted; the raw token is never stored.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct RefreshToken {
    /// Token row id.
    pub id: String,
    /// Owning user id.
    pub user_id: String,
    /// SHA-256 of the opaque refresh token.
    pub token_hash: Vec<u8>,
    /// Optional human label for the issuing device.
    pub device_label: Option<String>,
    /// Issue time (unix seconds).
    pub created_at: i64,
    /// Expiry time (unix seconds).
    pub expires_at: i64,
    /// Revocation time, or `None` while active.
    pub revoked_at: Option<i64>,
    /// Id of the token this one was rotated into, if any.
    pub rotated_to: Option<String>,
}

/// A pending GitHub web (authorization-code) login flow. Tracks one in-flight
/// browser round-trip; the GitHub `gh_state` keys the callback lookup.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct WebAuthFlow {
    /// Opaque flow id (primary key).
    pub flow_id: String,
    /// Random state echoed to GitHub and matched on the callback (CSRF).
    pub gh_state: String,
    /// Allow-listed redirect target (custom scheme or loopback http).
    pub redirect_uri: String,
    /// The client's own CSRF state, echoed back in the final redirect.
    pub app_state: String,
    /// base64url(SHA-256(code_verifier)) — PKCE binding for the app↔backend
    /// leg.
    pub code_challenge: String,
    /// Optional client device label, carried onto the issued refresh token.
    pub device_label: Option<String>,
    /// Creation time (unix seconds).
    pub created_at: i64,
    /// Expiry time (unix seconds).
    pub expires_at: i64,
    /// When the GitHub callback consumed the flow, or `None`.
    pub consumed_at: Option<i64>,
}

/// A one-time exchange code minted at the GitHub callback; the client trades it
/// (with the PKCE verifier) for a session at `/auth/web/exchange`.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct WebExchangeCode {
    /// The opaque one-time code (primary key).
    pub code: String,
    /// Owning user id.
    pub user_id: String,
    /// Device label to carry onto the issued refresh token.
    pub device_label: Option<String>,
    /// PKCE challenge copied from the flow; verified against the verifier.
    pub code_challenge: String,
    /// Creation time (unix seconds).
    pub created_at: i64,
    /// Expiry time (unix seconds).
    pub expires_at: i64,
    /// When the code was redeemed, or `None` while live.
    pub consumed_at: Option<i64>,
}

/// A pending GitHub device-flow authorization.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct DeviceFlow {
    /// Opaque handle handed to the client (the GitHub device code stays here).
    pub handle: String,
    /// The GitHub `device_code` used to poll GitHub server-side.
    pub github_device_code: String,
    /// Minimum poll interval GitHub asked for (seconds).
    pub interval_secs: i64,
    /// Creation time (unix seconds).
    pub created_at: i64,
    /// Expiry time (unix seconds).
    pub expires_at: i64,
    /// When the flow was authorized + consumed, or `None`.
    pub consumed_at: Option<i64>,
    /// Optional client-supplied device label, carried onto the issued refresh
    /// token at poll time so the user can tell sessions apart.
    pub device_label: Option<String>,
}
