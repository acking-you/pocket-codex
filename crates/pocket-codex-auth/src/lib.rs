//! GitHub-device-flow login and session management for the hosted backend.
//!
//! Responsibilities:
//! - drive GitHub's device flow (start → poll) one poll per client request;
//! - on authorization, upsert the user and issue a session: a short-lived HS256
//!   JWT (verified statelessly) plus an opaque refresh token (only its SHA-256
//!   is stored);
//! - rotate sessions on refresh and revoke on logout.
//!
//! All time is a caller-supplied unix-second `now`, so the service is
//! clock-injectable and the JWT/refresh lifetimes are explicit.

#![forbid(unsafe_code)]

mod error;
mod github;
mod jwt;

use base64::Engine as _;
pub use error::{AuthError, Result};
use github::{GitHub, PollResult};
pub use jwt::Claims;
use jwt::Jwt;
use pocket_codex_account_proto::{
    http::{
        DevicePollResponse, DevicePollStatus, DeviceStartResponse, SessionCredential,
        WebStartResponse,
    },
    key::SERVICE_NS_PREFIX,
};
use pocket_codex_store::{Store, User};
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

const MIN_GITHUB_POLL_INTERVAL_SECS: u64 = 5;
const MAX_GITHUB_POLL_INTERVAL_SECS: u64 = 300;

/// Configuration for [`Auth`].
pub struct Config {
    /// GitHub OAuth app client id (Device Flow enabled).
    pub github_client_id: String,
    /// GitHub OAuth app client secret. Required only for the web
    /// (authorization-code) flow; leave `None` to keep it disabled.
    pub github_client_secret: Option<String>,
    /// OAuth scope to request (e.g. `read:user`).
    pub github_scope: String,
    /// HS256 secret for signing session tokens.
    pub jwt_secret: String,
    /// Session (JWT) lifetime in seconds.
    pub jwt_ttl_secs: i64,
    /// Refresh-token lifetime in seconds.
    pub refresh_ttl_secs: i64,
    /// The backend's public OAuth callback URL
    /// (`https://<host>[:port]/auth/web/callback`). Required only for the web
    /// flow; must exactly match the GitHub OAuth app's registered callback URL.
    pub web_callback_url: Option<String>,
}

/// The web-flow-only secrets, present iff both the client secret and the public
/// callback URL are configured.
struct WebConfig {
    /// GitHub OAuth app client secret (backend-only).
    client_secret: String,
    /// Public callback URL handed to GitHub as `redirect_uri`.
    callback_url: String,
}

/// The auth service: device-flow login, session issue/refresh/logout, and
/// stateless JWT verification.
pub struct Auth {
    store: Store,
    github: GitHub,
    jwt: Jwt,
    jwt_ttl_secs: i64,
    refresh_ttl_secs: i64,
    scope: String,
    /// Web-flow secrets, or `None` when the authorization-code flow is
    /// disabled.
    web: Option<WebConfig>,
}

impl Auth {
    /// Build the auth service over a [`Store`] and configuration.
    pub fn new(store: Store, config: Config) -> Result<Self> {
        Self::new_with_github(store, config, GitHub::new)
    }

    #[cfg(test)]
    fn new_with_github_base(store: Store, config: Config, github_base_url: &str) -> Result<Self> {
        Self::new_with_github(store, config, |http, client_id| {
            GitHub::new_with_base_url(http, client_id, github_base_url)
        })
    }

    fn new_with_github(
        store: Store,
        config: Config,
        github: impl FnOnce(reqwest::Client, String) -> GitHub,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("pocket-codex")
            // Bound every GitHub call so a slow/black-holing upstream can't pin a
            // backend request (and its connection) indefinitely.
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;
        // The web (authorization-code) flow lights up only when BOTH the client
        // secret and the public callback URL are set; otherwise it stays off and
        // its endpoints return `WebDisabled` while the device flow keeps working.
        let web = match (config.github_client_secret, config.web_callback_url) {
            (Some(secret), Some(callback))
                if !secret.trim().is_empty() && !callback.trim().is_empty() =>
            {
                Some(WebConfig {
                    client_secret: secret,
                    callback_url: callback,
                })
            },
            _ => None,
        };
        Ok(Self {
            github: github(http, config.github_client_id),
            jwt: Jwt::new(config.jwt_secret.as_bytes()),
            jwt_ttl_secs: config.jwt_ttl_secs,
            refresh_ttl_secs: config.refresh_ttl_secs,
            scope: config.github_scope,
            web,
            store,
        })
    }

    /// Whether the web (authorization-code) login flow is configured + enabled.
    pub fn web_enabled(&self) -> bool {
        self.web.is_some()
    }

    /// Begin a device flow: ask GitHub for a code, stash the device code (and
    /// an optional client device label) under an opaque handle, and return
    /// the user code + verification URL.
    pub async fn device_start(
        &self,
        device_label: Option<&str>,
        now: i64,
    ) -> Result<DeviceStartResponse> {
        let dc = self.github.request_device_code(&self.scope).await?;
        let handle = Uuid::new_v4().simple().to_string();
        self.store
            .insert_device_flow(
                &handle,
                &dc.device_code,
                device_label,
                dc.interval,
                now,
                now + dc.expires_in,
            )
            .await?;
        Ok(DeviceStartResponse {
            user_code: dc.user_code,
            verification_uri: dc.verification_uri,
            poll_handle: handle,
            interval_secs: github_poll_interval(dc.interval.max(0) as u64),
            expires_in_secs: dc.expires_in.max(0) as u64,
        })
    }

    /// Poll a device flow once. On authorization, upsert the user, mark the
    /// flow consumed, and return a fresh session credential. The device label
    /// captured at `device_start` is carried onto the issued refresh token.
    pub async fn device_poll(&self, handle: &str, now: i64) -> Result<DevicePollResponse> {
        let flow = match self.store.device_flow(handle).await? {
            Some(f) if f.consumed_at.is_none() && f.expires_at > now => f,
            _ => return Ok(status_only(DevicePollStatus::Expired)),
        };
        match self.github.poll_token(&flow.github_device_code).await? {
            PollResult::Pending => Ok(status_only(DevicePollStatus::Pending)),
            PollResult::SlowDown {
                interval_secs,
            } => Ok(DevicePollResponse {
                status: DevicePollStatus::SlowDown,
                interval_secs: interval_secs.map(github_poll_interval),
                credential: None,
            }),
            PollResult::Expired => Ok(status_only(DevicePollStatus::Expired)),
            PollResult::Denied => Ok(status_only(DevicePollStatus::Denied)),
            PollResult::Authorized(access_token) => {
                let gh = self.github.fetch_user(&access_token).await?;
                let user = self.store.upsert_user(gh.id, &gh.login, now).await?;
                self.store.consume_device_flow(handle, now).await?;
                let (credential, _) = self
                    .issue_session(&user, flow.device_label.as_deref(), now)
                    .await?;
                Ok(DevicePollResponse {
                    status: DevicePollStatus::Authorized,
                    interval_secs: None,
                    credential: Some(credential),
                })
            },
        }
    }

    /// Begin a web (authorization-code) login: validate the client's
    /// `redirect_uri`, stash the flow (CSRF state + PKCE challenge), and return
    /// the GitHub authorization URL to open in a browser.
    pub async fn web_start(
        &self,
        redirect_uri: &str,
        app_state: &str,
        code_challenge: &str,
        device_label: Option<&str>,
        now: i64,
    ) -> Result<WebStartResponse> {
        let web = self.web.as_ref().ok_or(AuthError::WebDisabled)?;
        if !is_allowed_redirect(redirect_uri) {
            return Err(AuthError::BadRedirect);
        }
        let flow_id = Uuid::new_v4().simple().to_string();
        let gh_state = gen_state();
        self.store
            .insert_web_flow(
                &flow_id,
                &gh_state,
                redirect_uri,
                app_state,
                code_challenge,
                device_label,
                now,
                now + WEB_FLOW_TTL_SECS,
            )
            .await?;
        let authorize_url = self
            .github
            .authorize_url(&self.scope, &web.callback_url, &gh_state);
        Ok(WebStartResponse {
            authorize_url,
        })
    }

    /// Handle GitHub's redirect to the backend callback. Matches the `state`,
    /// consumes the flow once, exchanges the code for a GitHub token (using the
    /// client secret), upserts the user, and mints a single-use exchange code.
    /// Returns where to redirect the browser next (the client's `redirect_uri`
    /// with `?exchange_code=…&state=…`, or `?error=…&state=…`).
    ///
    /// An unknown/expired/duplicate `state` yields [`AuthError::BadWebState`]
    /// so the caller can render a generic page rather than redirect
    /// somewhere unvalidated.
    pub async fn web_callback(
        &self,
        code: Option<&str>,
        gh_state: &str,
        error: Option<&str>,
        now: i64,
    ) -> Result<WebCallbackRedirect> {
        let web = self.web.as_ref().ok_or(AuthError::WebDisabled)?;
        let flow = self
            .store
            .web_flow_by_state(gh_state)
            .await?
            .filter(|f| f.consumed_at.is_none() && f.expires_at > now)
            .ok_or(AuthError::BadWebState)?;
        // Consume once before doing any work: a duplicated callback (GitHub
        // retry, double-click) flips the row only for the first caller.
        if !self.store.consume_web_flow(&flow.flow_id, now).await? {
            return Err(AuthError::BadWebState);
        }
        // The user denied on GitHub (or GitHub reported an error): bounce the
        // app back with an error param so it can show a friendly message.
        if let Some(err) = error.filter(|e| !e.is_empty()) {
            return Ok(WebCallbackRedirect::error(&flow.redirect_uri, &flow.app_state, err));
        }
        let Some(code) = code.filter(|c| !c.is_empty()) else {
            return Ok(WebCallbackRedirect::error(
                &flow.redirect_uri,
                &flow.app_state,
                "missing_code",
            ));
        };
        let access = match self
            .github
            .exchange_code(&web.client_secret, code, &web.callback_url)
            .await
        {
            Ok(token) => token,
            Err(e) => {
                tracing::warn!(error = %e, "web flow: GitHub code exchange failed");
                return Ok(WebCallbackRedirect::error(
                    &flow.redirect_uri,
                    &flow.app_state,
                    "exchange_failed",
                ));
            },
        };
        let gh = self.github.fetch_user(&access).await?;
        let user = self.store.upsert_user(gh.id, &gh.login, now).await?;
        let exchange_code = Uuid::new_v4().simple().to_string();
        self.store
            .insert_web_exchange(
                &exchange_code,
                &user.id,
                flow.device_label.as_deref(),
                &flow.code_challenge,
                now,
                now + WEB_EXCHANGE_TTL_SECS,
            )
            .await?;
        Ok(WebCallbackRedirect::success(&flow.redirect_uri, &flow.app_state, &exchange_code))
    }

    /// Redeem a one-time exchange code (with its PKCE verifier) for a session,
    /// the same credential the device flow issues. The verifier must hash to
    /// the challenge captured at `web_start`, so a party that only
    /// intercepted the redirect (e.g. a custom-scheme hijacker) cannot
    /// redeem it.
    pub async fn web_exchange(
        &self,
        exchange_code: &str,
        code_verifier: &str,
        now: i64,
    ) -> Result<SessionCredential> {
        // The flow being configured is implied by a live exchange code, but
        // guard explicitly so a disabled backend never mints a session.
        let _ = self.web.as_ref().ok_or(AuthError::WebDisabled)?;
        let entry = self
            .store
            .active_web_exchange(exchange_code, now)
            .await?
            .ok_or(AuthError::BadExchange)?;
        if pkce_challenge(code_verifier) != entry.code_challenge {
            return Err(AuthError::BadExchange);
        }
        // Redeem once: only the caller that flips the row mints a session.
        if !self.store.consume_web_exchange(exchange_code, now).await? {
            return Err(AuthError::BadExchange);
        }
        let user = self
            .store
            .user(&entry.user_id)
            .await?
            .ok_or(AuthError::BadExchange)?;
        let (credential, _) = self
            .issue_session(&user, entry.device_label.as_deref(), now)
            .await?;
        Ok(credential)
    }

    /// Exchange a valid refresh token for a new session, rotating (revoking)
    /// the presented token.
    pub async fn refresh(&self, refresh_token: &str, now: i64) -> Result<SessionCredential> {
        let hash = hash_token(refresh_token);
        let existing = match self.store.active_refresh_token(&hash, now).await? {
            Some(token) => token,
            None => {
                self.handle_inactive_refresh(&hash, now).await?;
                return Err(AuthError::BadRefresh);
            },
        };
        // Consume the presented token atomically: the conditional UPDATE flips at
        // most one row, so under concurrent refreshes only the caller that flips
        // it (rows == 1) mints a session. Single-use rotation therefore holds even
        // though the read and the write are separate SQLite statements.
        if self
            .store
            .revoke_refresh_token(&existing.id, now, None)
            .await?
            != 1
        {
            return Err(AuthError::BadRefresh);
        }
        let user = self
            .store
            .user(&existing.user_id)
            .await?
            .ok_or(AuthError::BadRefresh)?;
        let (credential, new_refresh_id) = self
            .issue_session(&user, existing.device_label.as_deref(), now)
            .await?;
        // Record the rotation chain (best-effort; the single-use gate above
        // already holds regardless) so a later replay of `existing` is seen as a
        // benign lost-response retry rather than theft.
        let _ = self
            .store
            .set_rotated_to(&existing.id, &new_refresh_id)
            .await;
        Ok(credential)
    }

    /// A presented refresh token was not active. Decide whether this is theft
    /// (a replay of a long-revoked / logged-out token → revoke the whole
    /// family per RFC 6819) or a benign lost-response retry (a token
    /// rotated within the grace window → reject only this request, leaving
    /// the user's other sessions alone so a transient network failure can't
    /// log every device out).
    async fn handle_inactive_refresh(&self, hash: &[u8], now: i64) -> Result<()> {
        let Some(seen) = self.store.refresh_token_by_hash(hash).await? else {
            return Ok(());
        };
        if is_lost_response_retry(seen.rotated_to.as_deref(), seen.revoked_at, now) {
            tracing::info!(
                user_id = %seen.user_id,
                "refresh token replayed within rotation grace window; \
                 rejecting without revoking the user's other sessions"
            );
        } else {
            self.store
                .revoke_user_refresh_tokens(&seen.user_id, now)
                .await?;
            tracing::warn!(
                user_id = %seen.user_id,
                "refresh-token reuse detected; revoked all of the user's sessions"
            );
        }
        Ok(())
    }

    /// Revoke a refresh token (logout of one device). Unknown tokens are a
    /// no-op.
    pub async fn logout(&self, refresh_token: &str, now: i64) -> Result<()> {
        let hash = hash_token(refresh_token);
        if let Some(existing) = self.store.active_refresh_token(&hash, now).await? {
            self.store
                .revoke_refresh_token(&existing.id, now, None)
                .await?;
        }
        Ok(())
    }

    /// Statelessly verify a session token, returning its claims.
    pub fn verify(&self, token: &str) -> Result<Claims> {
        Ok(self.jwt.verify(token)?)
    }

    /// Issue a session and return it alongside the new refresh token's row id
    /// (so the caller can record a rotation chain).
    async fn issue_session(
        &self,
        user: &User,
        device_label: Option<&str>,
        now: i64,
    ) -> Result<(SessionCredential, String)> {
        let claims = Claims {
            sub: user.id.clone(),
            ns: format!("{SERVICE_NS_PREFIX}:{}", user.id),
            login: user.github_login.clone(),
            gh_id: user.github_id,
            iat: now,
            exp: now + self.jwt_ttl_secs,
            jti: Uuid::new_v4().simple().to_string(),
        };
        let token = self.jwt.issue(&claims)?;
        let refresh = gen_refresh_token();
        let refresh_id = self
            .store
            .insert_refresh_token(
                &user.id,
                &hash_token(&refresh),
                device_label,
                now,
                now + self.refresh_ttl_secs,
            )
            .await?;
        Ok((
            SessionCredential {
                token,
                refresh_token: refresh,
                expires_in_secs: self.jwt_ttl_secs.max(0) as u64,
                login: user.github_login.clone(),
                account_id: Some(user.github_id.to_string()),
            },
            refresh_id,
        ))
    }
}

/// Lifetime of a started web login flow — the browser round-trip (open GitHub,
/// authorize, redirect back). Generous enough for a real sign-in, short enough
/// that abandoned flows are purged promptly.
const WEB_FLOW_TTL_SECS: i64 = 600;

/// Lifetime of a one-time exchange code. Tighter than the flow: the client
/// already has the redirect in hand and only needs one round-trip to redeem it.
const WEB_EXCHANGE_TTL_SECS: i64 = 300;

/// Where to send the browser after the GitHub callback: the client's
/// `redirect_uri` with the result appended as query params.
pub struct WebCallbackRedirect {
    /// The fully-built redirect location (custom scheme or loopback http).
    pub location: String,
}

impl WebCallbackRedirect {
    /// A success redirect carrying the one-time exchange code + the app's
    /// state.
    fn success(redirect_uri: &str, app_state: &str, exchange_code: &str) -> Self {
        Self {
            location: build_redirect(redirect_uri, &[
                ("exchange_code", exchange_code),
                ("state", app_state),
            ]),
        }
    }

    /// An error redirect carrying an error code + the app's state.
    fn error(redirect_uri: &str, app_state: &str, error: &str) -> Self {
        Self {
            location: build_redirect(redirect_uri, &[("error", error), ("state", app_state)]),
        }
    }
}

/// Append query params to a (query-less) redirect target. Works for both custom
/// schemes (`pocketcodex://auth`) and loopback http URLs; values are
/// percent-encoded.
fn build_redirect(redirect_uri: &str, params: &[(&str, &str)]) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().copied())
        .finish();
    let sep = if redirect_uri.contains('?') { '&' } else { '?' };
    format!("{redirect_uri}{sep}{query}")
}

/// Whether a client `redirect_uri` is allowed as a web-flow callback target.
/// Only the app's custom scheme (mobile deep link) and loopback http
/// (desktop / CLI) are permitted, so the one-time exchange code can never be
/// redirected to an arbitrary origin.
fn is_allowed_redirect(redirect_uri: &str) -> bool {
    let Ok(url) = url::Url::parse(redirect_uri) else {
        return false;
    };
    match url.scheme() {
        // Mobile deep link back into the app.
        "pocketcodex" => true,
        // Desktop / CLI loopback listener (no TLS needed — it never leaves the
        // device). Reject any other http host so a token can't be exfiltrated.
        "http" => matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]" | "::1")),
        _ => false,
    }
}

/// A random, URL-safe CSRF state token.
fn gen_state() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The PKCE challenge for a verifier: `base64url(SHA-256(verifier))`, no pad.
fn pkce_challenge(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// How recently a token must have been rotated for a replay of it to be treated
/// as a benign lost-response retry (vs theft). A client that successfully
/// rotated but lost the response retries with the old token within seconds; a
/// stolen token is typically replayed much later.
const REUSE_GRACE_SECS: i64 = 60;

/// Whether replaying an inactive refresh token looks like a benign
/// lost-response retry rather than theft: it must have been *rotated* (a
/// successor exists, so not a logout) and revoked within the grace window. Pure
/// so the policy is unit-testable without a store.
fn is_lost_response_retry(rotated_to: Option<&str>, revoked_at: Option<i64>, now: i64) -> bool {
    rotated_to.is_some() && revoked_at.is_some_and(|revoked| now - revoked <= REUSE_GRACE_SECS)
}

fn status_only(status: DevicePollStatus) -> DevicePollResponse {
    DevicePollResponse {
        status,
        interval_secs: None,
        credential: None,
    }
}

fn github_poll_interval(interval_secs: u64) -> u64 {
    interval_secs.clamp(MIN_GITHUB_POLL_INTERVAL_SECS, MAX_GITHUB_POLL_INTERVAL_SECS)
}

fn gen_refresh_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Error, ErrorKind},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
    };

    use super::*;

    #[test]
    fn lost_response_retry_only_within_grace_of_a_rotation() {
        let rotated_at = 1_000;
        // A token rotated within the grace window → benign lost-response retry,
        // so refresh rejects this one request without nuking the family.
        assert!(is_lost_response_retry(Some("next"), Some(rotated_at), rotated_at));
        assert!(is_lost_response_retry(
            Some("next"),
            Some(rotated_at),
            rotated_at + REUSE_GRACE_SECS
        ));
        // Rotated long ago → treat the replay as theft (revoke the family).
        assert!(!is_lost_response_retry(
            Some("next"),
            Some(rotated_at),
            rotated_at + REUSE_GRACE_SECS + 1
        ));
        // Revoked via logout (no successor) → theft, even if recent.
        assert!(!is_lost_response_retry(None, Some(rotated_at), rotated_at));
        // Never revoked → not an inactive-token replay at all.
        assert!(!is_lost_response_retry(Some("next"), None, rotated_at));
    }

    #[test]
    fn redirect_allowlist_accepts_scheme_and_loopback_only() {
        // Mobile custom scheme.
        assert!(is_allowed_redirect("pocketcodex://auth"));
        assert!(is_allowed_redirect("pocketcodex://auth/callback"));
        // Desktop / CLI loopback.
        assert!(is_allowed_redirect("http://127.0.0.1:54321/callback"));
        assert!(is_allowed_redirect("http://localhost:8080"));
        // Rejected: arbitrary origins, https non-loopback, other schemes.
        assert!(!is_allowed_redirect("https://evil.example/callback"));
        assert!(!is_allowed_redirect("http://evil.example/callback"));
        assert!(!is_allowed_redirect("https://127.0.0.1/callback"));
        assert!(!is_allowed_redirect("javascript:alert(1)"));
        assert!(!is_allowed_redirect("not a url"));
    }

    #[test]
    fn pkce_challenge_matches_rfc7636_vector() {
        // RFC 7636 Appendix B test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(pkce_challenge(verifier), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn build_redirect_appends_query_for_scheme_and_url() {
        assert_eq!(
            build_redirect("pocketcodex://auth", &[("exchange_code", "abc"), ("state", "s1")]),
            "pocketcodex://auth?exchange_code=abc&state=s1"
        );
        // A target that already has a query gets `&`.
        assert_eq!(
            build_redirect("http://localhost:8080/cb?x=1", &[("error", "denied")]),
            "http://localhost:8080/cb?x=1&error=denied"
        );
        // Values are percent-encoded.
        assert_eq!(
            build_redirect("pocketcodex://auth", &[("error", "exchange failed")]),
            "pocketcodex://auth?error=exchange+failed"
        );
    }

    #[test]
    fn github_poll_interval_bounds_provider_values() {
        assert_eq!(github_poll_interval(0), 5);
        assert_eq!(github_poll_interval(11), 11);
        assert_eq!(github_poll_interval(u64::MAX), 300);
    }

    fn cfg(web: bool) -> Config {
        Config {
            github_client_id: "Iv1.test".to_string(),
            github_client_secret: web.then(|| "client-secret".to_string()),
            github_scope: "read:user".to_string(),
            jwt_secret: "x".repeat(32),
            jwt_ttl_secs: 3600,
            refresh_ttl_secs: 1000,
            web_callback_url: web.then(|| "https://lb7666.top:8443/auth/web/callback".to_string()),
        }
    }

    #[tokio::test]
    async fn web_start_gates_on_config_and_redirect_allowlist() {
        // Disabled flow → WebDisabled before touching the store, even for an
        // otherwise-valid redirect.
        let store = Store::connect("sqlite::memory:").await.expect("store");
        let disabled = Auth::new(store, cfg(false)).expect("auth");
        assert!(!disabled.web_enabled());
        assert!(matches!(
            disabled
                .web_start("pocketcodex://auth", "s", "c", None, 0)
                .await,
            Err(AuthError::WebDisabled)
        ));

        // Enabled flow but a disallowed redirect → BadRedirect (returned before
        // any store write, so the in-memory pool is never queried).
        let store = Store::connect("sqlite::memory:").await.expect("store");
        let enabled = Auth::new(store, cfg(true)).expect("auth");
        assert!(enabled.web_enabled());
        assert!(matches!(
            enabled
                .web_start("https://evil.example/cb", "s", "c", None, 0)
                .await,
            Err(AuthError::BadRedirect)
        ));
    }

    #[tokio::test]
    async fn device_flow_authorizes_after_github_approval() {
        let github = FakeGithub::spawn().await;
        let store = Store::connect("sqlite::memory:").await.expect("store");
        let auth = Auth::new_with_github_base(store, cfg(false), github.base_url()).expect("auth");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_secs() as i64;

        let start = auth
            .device_start(Some("laptop"), now)
            .await
            .expect("device start");
        assert_eq!(start.user_code, "ABCD-EFGH");
        assert_eq!(start.verification_uri, format!("{}/login/device", github.base_url()));
        assert_eq!(start.interval_secs, 5);

        let pending = auth
            .device_poll(&start.poll_handle, now + 1)
            .await
            .expect("pending poll");
        assert_eq!(pending.status, DevicePollStatus::Pending);
        assert!(pending.credential.is_none());

        let slow_down = auth
            .device_poll(&start.poll_handle, now + 2)
            .await
            .expect("slow_down poll");
        assert_eq!(slow_down.status, DevicePollStatus::SlowDown);
        assert_eq!(slow_down.interval_secs, Some(11));
        assert!(slow_down.credential.is_none());

        let authorized = auth
            .device_poll(&start.poll_handle, now + 3)
            .await
            .expect("authorized poll");
        assert_eq!(authorized.status, DevicePollStatus::Authorized);
        assert_eq!(authorized.interval_secs, None);
        let credential = authorized.credential.expect("credential");
        assert_eq!(credential.login, "octocat");
        assert_eq!(credential.account_id.as_deref(), Some("42"));
        let claims = auth.verify(&credential.token).expect("session jwt");
        assert_eq!(claims.login, "octocat");
        assert_eq!(claims.gh_id, 42);
        assert_eq!(github.token_polls(), 3);

        let consumed = auth
            .device_poll(&start.poll_handle, now + 4)
            .await
            .expect("consumed poll");
        assert_eq!(consumed.status, DevicePollStatus::Expired);
        assert!(consumed.credential.is_none());
    }

    struct FakeGithub {
        base_url: String,
        token_polls: Arc<AtomicUsize>,
    }

    impl FakeGithub {
        async fn spawn() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind fake github");
            let addr = listener.local_addr().expect("fake github addr");
            let base_url = format!("http://{addr}");
            let token_polls = Arc::new(AtomicUsize::new(0));
            let server_base = base_url.clone();
            let server_polls = token_polls.clone();
            tokio::spawn(async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let base = server_base.clone();
                    let polls = server_polls.clone();
                    tokio::spawn(async move {
                        if let Ok((method, target, body)) = read_http_request(&mut stream).await {
                            let (status, json) =
                                fake_github_response(&base, &polls, &method, &target, &body);
                            let response = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: \
                                 application/json\r\nContent-Length: {}\r\nConnection: \
                                 close\r\n\r\n{json}",
                                json.len()
                            );
                            let _ = stream.write_all(response.as_bytes()).await;
                            let _ = stream.shutdown().await;
                        }
                    });
                }
            });
            Self {
                base_url,
                token_polls,
            }
        }

        fn base_url(&self) -> &str {
            &self.base_url
        }

        fn token_polls(&self) -> usize {
            self.token_polls.load(Ordering::SeqCst)
        }
    }

    async fn read_http_request(
        stream: &mut TcpStream,
    ) -> std::io::Result<(String, String, String)> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        let header_end = loop {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return Err(Error::new(ErrorKind::UnexpectedEof, "request closed before headers"));
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
            if buf.len() > 16 * 1024 {
                return Err(Error::new(ErrorKind::InvalidData, "request headers too large"));
            }
        };
        let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut lines = headers.lines();
        let request_line = lines
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing request line"))?;
        let mut parts = request_line.split_whitespace();
        let method = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing method"))?
            .to_string();
        let target = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing target"))?
            .to_string();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        while buf.len() < header_end + content_length {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return Err(Error::new(ErrorKind::UnexpectedEof, "request closed before body"));
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let body =
            String::from_utf8_lossy(&buf[header_end..header_end + content_length]).to_string();
        Ok((method, target, body))
    }

    fn fake_github_response(
        base_url: &str,
        token_polls: &AtomicUsize,
        method: &str,
        target: &str,
        body: &str,
    ) -> (&'static str, String) {
        match (method, target) {
            ("POST", "/login/device/code") if body.contains("client_id=Iv1.test") => (
                "200 OK",
                format!(
                    r#"{{"device_code":"device-123","user_code":"ABCD-EFGH","verification_uri":"{base_url}/login/device","expires_in":900,"interval":1}}"#
                ),
            ),
            ("POST", "/login/oauth/access_token") if body.contains("device_code=device-123") => {
                match token_polls.fetch_add(1, Ordering::SeqCst) {
                    0 => ("200 OK", r#"{"error":"authorization_pending"}"#.to_string()),
                    1 => ("200 OK", r#"{"error":"slow_down","interval":11}"#.to_string()),
                    _ => ("200 OK", r#"{"access_token":"gh-access"}"#.to_string()),
                }
            },
            ("GET", "/user") => ("200 OK", r#"{"id":42,"login":"octocat"}"#.to_string()),
            _ => (
                "404 Not Found",
                format!(r#"{{"error":"unexpected {method} {target} body={body}"}}"#),
            ),
        }
    }
}
