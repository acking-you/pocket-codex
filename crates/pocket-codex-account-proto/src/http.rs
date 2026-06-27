//! JSON bodies for the backend's HTTP API (served over HTTPS).
//!
//! Auth is GitHub Device Flow, mediated by the backend (which holds the OAuth
//! client secret): `start` returns a user code + verification URL, the client
//! polls until the backend has a session, then uses the bearer token for
//! `/v1/*` and the broker tunnel.

use pocket_codex_core::service::{ServiceId, ServiceKind};
use serde::{Deserialize, Serialize};

/// Request body for `POST /auth/device/start`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStartRequest {
    /// Optional human label for the session (e.g. hostname), stored with the
    /// refresh token so the user can tell devices apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
}

/// Response to `POST /auth/device/start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStartResponse {
    /// Code the user types at [`Self::verification_uri`].
    pub user_code: String,
    /// URL the user opens to enter [`Self::user_code`].
    pub verification_uri: String,
    /// Opaque handle the client passes back to `poll` (never the raw GitHub
    /// device code).
    pub poll_handle: String,
    /// Minimum seconds the client must wait between polls.
    pub interval_secs: u64,
    /// Seconds until this device flow expires.
    pub expires_in_secs: u64,
}

/// Request body for `POST /auth/device/poll`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicePollRequest {
    /// The handle returned by `start`.
    pub poll_handle: String,
}

/// Outcome of a single device-flow poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevicePollStatus {
    /// The user has not authorized yet; keep polling.
    Pending,
    /// Polling too fast; back off then keep polling.
    SlowDown,
    /// Authorized — [`DevicePollResponse::credential`] is set.
    Authorized,
    /// The flow expired; restart from `start`.
    Expired,
    /// The user denied the request.
    Denied,
}

/// Response to `POST /auth/device/poll`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicePollResponse {
    /// Current status of the flow.
    pub status: DevicePollStatus,
    /// The issued session, present iff [`DevicePollStatus::Authorized`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<SessionCredential>,
}

/// A backend-issued session: a short-lived bearer token plus an opaque,
/// long-lived refresh token and the GitHub identity (for display).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCredential {
    /// Bearer token (JWT) for `/v1/*` and the broker `HELLO`.
    pub token: String,
    /// Opaque refresh token; exchange via `/auth/refresh` when the bearer
    /// expires.
    pub refresh_token: String,
    /// Bearer-token lifetime in seconds.
    pub expires_in_secs: u64,
    /// GitHub login/handle of the authenticated user.
    pub login: String,
    /// GitHub account id (opaque), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

/// Request body for `POST /auth/refresh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshRequest {
    /// The current refresh token (rotated on success).
    pub refresh_token: String,
}

/// Response to `POST /auth/refresh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshResponse {
    /// The new, rotated session.
    pub credential: SessionCredential,
}

/// Request body for `POST /auth/logout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogoutRequest {
    /// The refresh token to revoke.
    pub refresh_token: String,
}

/// Response to `GET /v1/me`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeResponse {
    /// GitHub login/handle.
    pub login: String,
    /// GitHub account id (opaque), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

/// One service in an account's listing — the per-user view, with the internal
/// `pcxu:<user>` prefix already stripped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceEntry {
    /// Device id segment.
    pub device: String,
    /// Service kind.
    pub kind: ServiceKind,
    /// Instance name segment.
    pub name: String,
}

impl ServiceEntry {
    /// The local (self-host-shaped) [`ServiceId`] the app/CLI uses to identify
    /// this service in its UI and session layer.
    pub fn to_service_id(&self) -> ServiceId {
        ServiceId::new(&self.device, self.kind, &self.name)
    }
}

/// Response to `GET /v1/services`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicesResponse {
    /// The account's services discovered on the relay.
    pub services: Vec<ServiceEntry>,
}
