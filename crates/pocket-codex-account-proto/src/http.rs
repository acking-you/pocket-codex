//! JSON bodies for the backend's HTTP API (served over HTTPS).
//!
//! Auth is GitHub login — Device Flow or the browser-redirect flow — mediated
//! by the backend, which holds the OAuth client secret. `start` returns a user
//! code and a verification URL, the client polls until the backend has a
//! session, then uses the bearer token for every `/v1/*` call.
//!
//! [`RelayCredentialResponse`] is where a client stops needing the backend: it
//! carries the relay address and a credential, and everything after it is
//! client↔relay.

use pocket_codex_core::service::ServiceKind;
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
    /// Updated minimum seconds before the next poll, present when the provider
    /// reports [`DevicePollStatus::SlowDown`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    /// The issued session, present iff [`DevicePollStatus::Authorized`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<SessionCredential>,
}

/// Request body for `POST /auth/web/start` (the browser-redirect /
/// authorization-code login flow).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebStartRequest {
    /// Where the backend sends the browser at the end of the flow: the app's
    /// custom-scheme deep link (`pocketcodex://…`) or a loopback `http://` URL.
    /// The backend rejects anything else so the one-time exchange code can
    /// never be redirected off-device.
    pub redirect_uri: String,
    /// The client's own CSRF state; echoed back in the final redirect so the
    /// client can confirm the response matches its request.
    pub state: String,
    /// PKCE `base64url(SHA-256(code_verifier))`. Binds the eventual exchange
    /// code to this client (which alone holds the verifier).
    pub code_challenge: String,
    /// Optional human label for the session (e.g. hostname), carried onto the
    /// issued refresh token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
}

/// Response to `POST /auth/web/start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebStartResponse {
    /// The GitHub authorization URL to open in a browser. After the user
    /// authorizes, the backend redirects the browser to the requested
    /// `redirect_uri` with `?exchange_code=…&state=…`.
    pub authorize_url: String,
}

/// Request body for `POST /auth/web/exchange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebExchangeRequest {
    /// The one-time code delivered to the client via the final redirect.
    pub exchange_code: String,
    /// The PKCE code verifier whose challenge was sent at `web/start`.
    pub code_verifier: String,
}

/// Response to `POST /auth/web/exchange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebExchangeResponse {
    /// The issued session (same shape as the device flow).
    pub credential: SessionCredential,
}

/// A backend-issued session: a short-lived bearer token plus an opaque,
/// long-lived refresh token and the GitHub identity (for display).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCredential {
    /// Bearer token (JWT) for every `/v1/*` call.
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

/// Read a session token's `exp` claim (unix seconds), or `None` if it is not a
/// JWT with a numeric `exp`.
///
/// The signature is NOT verified, and deliberately so: only the backend holds
/// the key. A client reads `exp` to decide whether to refresh *before* spending
/// a request, so a forged token would at worst cause a needless refresh — and
/// the backend rejects it either way. Never treat a token as authentic because
/// this returned a value.
pub fn session_token_exp(token: &str) -> Option<i64> {
    use base64::Engine as _;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("exp")?.as_i64()
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

/// Response to `GET /v1/services`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicesResponse {
    /// The account's services discovered on the relay.
    pub services: Vec<ServiceEntry>,
}

/// Response to `GET /v1/relay` — everything a client needs to talk to the relay
/// itself, so it can register and subscribe WITHOUT the backend on the data
/// path.
///
/// The credential is a short-lived pb-mapper temporary credential, minted by
/// the backend under its administrator key. The relay confines it to its own
/// namespace, so one account can neither see nor address another's services
/// even though both dial the same relay.
///
/// It is deliberately per-ACCOUNT rather than per-device: a temporary
/// credential's namespace is its own key id, so two devices holding different
/// credentials could not see each other's services — which is the whole point
/// of the product. Devices on one account share a credential and therefore a
/// namespace; isolation is between accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayCredentialResponse {
    /// `host:port` of the relay to dial.
    pub relay_addr: String,
    /// The `pbmt1_…` credential to present.
    pub credential: String,
    /// Unix seconds at which the relay stops accepting it. Clients should
    /// re-request before this; the backend renews rather than re-minting (which
    /// keeps the credential string identical), so asking again early is cheap.
    pub expires_at: u64,
    /// The caller's account id, already sanitised into a key segment.
    ///
    /// Clients build their own relay keys from it via
    /// [`NamespacedServiceId::new`] — the backend no longer prepends the
    /// namespace on their behalf, because it no longer sees their traffic. It
    /// is still the backend that DECIDES the value, from the verified
    /// token, so a client cannot name another account's namespace and have
    /// the relay honour it: the credential is confined to one namespace
    /// regardless of the key string presented.
    ///
    /// [`NamespacedServiceId::new`]: crate::key::NamespacedServiceId::new
    pub namespace: String,
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;

    fn token_with_payload(payload: &[u8]) -> String {
        let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("h.{body}.s")
    }

    #[test]
    fn session_token_exp_reads_the_exp_claim() {
        let token = token_with_payload(br#"{"exp":1700000000}"#);
        assert_eq!(session_token_exp(&token), Some(1_700_000_000));
    }

    #[test]
    fn session_token_exp_declines_anything_it_cannot_read() {
        // Every one of these reaches the caller as "I don't know when this
        // expires", which is the safe answer: the caller refreshes rather than
        // assuming a token is good.
        assert_eq!(session_token_exp("not-a-jwt"), None);
        assert_eq!(session_token_exp(""), None);
        assert_eq!(session_token_exp("h.!!!not-base64!!!.s"), None);
        assert_eq!(session_token_exp(&token_with_payload(b"not json")), None);
        assert_eq!(session_token_exp(&token_with_payload(b"{}")), None);
        // A non-numeric `exp` is malformed, not a date to coerce.
        assert_eq!(session_token_exp(&token_with_payload(br#"{"exp":"soon"}"#)), None);
    }
}
