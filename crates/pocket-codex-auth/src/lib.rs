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
    /// (a replay of a long-revoked / logged-out token → revoke the family
    /// issued up to now, per RFC 6819) or a benign retry that must not log
    /// the user out.
    ///
    /// # Why "already revoked" is not evidence of theft
    ///
    /// A client whose refresh fails does not stop; it retries, for as long as
    /// it is running. So the SECOND replay of a token is expected whenever
    /// the first one was rejected — and treating each arrival as fresh
    /// abuse is what turned one lost race into a permanent lockout on
    /// lb7666.top (2026-08-30): every retry re-revoked the family,
    /// including any session the user had just created by signing in again,
    /// at ~10 events/minute for as long as the client stayed up.
    ///
    /// The fix is to make the response idempotent. A token that is already
    /// revoked has already had whatever consequence it deserved, so
    /// replaying it is rejected — the caller still gets
    /// [`AuthError::BadRefresh`] — without revoking anything a second time.
    /// Only the FIRST sighting of an inactive token can revoke a family,
    /// which is the only sighting that carries new information.
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
        } else if seen.rotated_to.is_none() {
            // Revoked with NO successor: either a logout, or a family revocation
            // this same code performed earlier. Both mean the consequence has
            // already been applied, so rejecting is right but revoking again is
            // not — re-revoking on every retry is what makes an account
            // unrecoverable while a client keeps trying.
            //
            // A rotated-but-stale token falls through to the branch below instead:
            // it still carries new information the first time it is seen, because
            // its successor may be live in someone else's hands.
            tracing::debug!(
                user_id = %seen.user_id,
                revoked_at = ?seen.revoked_at,
                "replay of an already-settled refresh token; rejecting without re-revoking"
            );
        } else {
            // Scoped to sessions that already existed: a client still retrying the
            // compromised token must not be able to revoke the session the user
            // creates by signing in after this.
            self.store
                .revoke_user_refresh_tokens_issued_through(&seen.user_id, now, now)
                .await?;
            tracing::warn!(
                user_id = %seen.user_id,
                "refresh-token reuse detected; revoked the user's existing sessions"
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
mod tests;
