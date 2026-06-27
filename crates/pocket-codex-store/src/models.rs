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
