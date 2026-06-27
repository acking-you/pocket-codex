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
    http::{DevicePollResponse, DevicePollStatus, DeviceStartResponse, SessionCredential},
    key::SERVICE_NS_PREFIX,
};
use pocket_codex_store::{Store, User};
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

/// Configuration for [`Auth`].
pub struct Config {
    /// GitHub OAuth app client id (Device Flow enabled).
    pub github_client_id: String,
    /// OAuth scope to request (e.g. `read:user`).
    pub github_scope: String,
    /// HS256 secret for signing session tokens.
    pub jwt_secret: String,
    /// Session (JWT) lifetime in seconds.
    pub jwt_ttl_secs: i64,
    /// Refresh-token lifetime in seconds.
    pub refresh_ttl_secs: i64,
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
}

impl Auth {
    /// Build the auth service over a [`Store`] and configuration.
    pub fn new(store: Store, config: Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("pocket-codex")
            // Bound every GitHub call so a slow/black-holing upstream can't pin a
            // backend request (and its connection) indefinitely.
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            github: GitHub::new(http, config.github_client_id),
            jwt: Jwt::new(config.jwt_secret.as_bytes()),
            jwt_ttl_secs: config.jwt_ttl_secs,
            refresh_ttl_secs: config.refresh_ttl_secs,
            scope: config.github_scope,
            store,
        })
    }

    /// Begin a device flow: ask GitHub for a code, stash the device code under
    /// an opaque handle, and return the user code + verification URL.
    pub async fn device_start(&self, now: i64) -> Result<DeviceStartResponse> {
        let dc = self.github.request_device_code(&self.scope).await?;
        let handle = Uuid::new_v4().simple().to_string();
        self.store
            .insert_device_flow(&handle, &dc.device_code, dc.interval, now, now + dc.expires_in)
            .await?;
        Ok(DeviceStartResponse {
            user_code: dc.user_code,
            verification_uri: dc.verification_uri,
            poll_handle: handle,
            interval_secs: dc.interval.max(0) as u64,
            expires_in_secs: dc.expires_in.max(0) as u64,
        })
    }

    /// Poll a device flow once. On authorization, upsert the user, mark the
    /// flow consumed, and return a fresh session credential.
    pub async fn device_poll(
        &self,
        handle: &str,
        device_label: Option<&str>,
        now: i64,
    ) -> Result<DevicePollResponse> {
        let flow = match self.store.device_flow(handle).await? {
            Some(f) if f.consumed_at.is_none() && f.expires_at > now => f,
            _ => return Ok(status_only(DevicePollStatus::Expired)),
        };
        match self.github.poll_token(&flow.github_device_code).await? {
            PollResult::Pending => Ok(status_only(DevicePollStatus::Pending)),
            PollResult::SlowDown => Ok(status_only(DevicePollStatus::SlowDown)),
            PollResult::Expired => Ok(status_only(DevicePollStatus::Expired)),
            PollResult::Denied => Ok(status_only(DevicePollStatus::Denied)),
            PollResult::Authorized(access_token) => {
                let gh = self.github.fetch_user(&access_token).await?;
                let user = self.store.upsert_user(gh.id, &gh.login, now).await?;
                self.store.consume_device_flow(handle, now).await?;
                let credential = self.issue_session(&user, device_label, now).await?;
                Ok(DevicePollResponse {
                    status: DevicePollStatus::Authorized,
                    credential: Some(credential),
                })
            },
        }
    }

    /// Exchange a valid refresh token for a new session, rotating (revoking)
    /// the presented token.
    pub async fn refresh(&self, refresh_token: &str, now: i64) -> Result<SessionCredential> {
        let hash = hash_token(refresh_token);
        let existing = match self.store.active_refresh_token(&hash, now).await? {
            Some(token) => token,
            None => {
                // A token that exists but is no longer active was already
                // rotated/revoked; a replay of it signals theft — revoke the
                // whole family (RFC 6819 reuse mitigation) before rejecting.
                if let Some(seen) = self.store.refresh_token_by_hash(&hash).await? {
                    self.store
                        .revoke_user_refresh_tokens(&seen.user_id, now)
                        .await?;
                    tracing::warn!(
                        user_id = %seen.user_id,
                        "refresh-token reuse detected; revoked all of the user's sessions"
                    );
                }
                return Err(AuthError::BadRefresh);
            },
        };
        // Consume the presented token atomically: the conditional UPDATE flips at
        // most one row, so under concurrent refreshes only the caller that flips
        // it (rows == 1) mints a session. Single-use rotation therefore holds even
        // though the read and the write are separate SQLite statements.
        if self.store.revoke_refresh_token(&existing.id, now, None).await? != 1 {
            return Err(AuthError::BadRefresh);
        }
        let user = self
            .store
            .user(&existing.user_id)
            .await?
            .ok_or(AuthError::BadRefresh)?;
        self.issue_session(&user, existing.device_label.as_deref(), now)
            .await
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

    async fn issue_session(
        &self,
        user: &User,
        device_label: Option<&str>,
        now: i64,
    ) -> Result<SessionCredential> {
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
        self.store
            .insert_refresh_token(
                &user.id,
                &hash_token(&refresh),
                device_label,
                now,
                now + self.refresh_ttl_secs,
            )
            .await?;
        Ok(SessionCredential {
            token,
            refresh_token: refresh,
            expires_in_secs: self.jwt_ttl_secs.max(0) as u64,
            login: user.github_login.clone(),
            account_id: Some(user.github_id.to_string()),
        })
    }
}

fn status_only(status: DevicePollStatus) -> DevicePollResponse {
    DevicePollResponse {
        status,
        credential: None,
    }
}

fn gen_refresh_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
