//! Hosted-account engine: GitHub device-flow login, session persistence and
//! refresh, identity, and the per-account services listing — all over the
//! backend HTTP API. The app holds only the backend-issued session token (a
//! JWT, persisted in the same 0600 `config.toml` as the relay key) and the
//! opaque refresh token; it never sees the relay key.
//!
//! Pure async logic (no flutter_rust_bridge); the `api` layer drives it on the
//! engine runtime.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use pocket_codex_account_proto::http::{
    DevicePollRequest, DevicePollResponse, DevicePollStatus, DeviceStartRequest,
    DeviceStartResponse, LogoutRequest, MeResponse, RefreshRequest, RefreshResponse, ServiceEntry,
};
use pocket_codex_core::config::Config;

use crate::engine::config::{load_config, save_config};

/// Compile-time default backend base URL.
pub const DEFAULT_BACKEND: &str = "https://lb7666.top:8443";

/// The persisted backend base URL, or the built-in default.
pub fn backend_base(config: &Config) -> String {
    config
        .account_backend()
        .map(ToString::to_string)
        .unwrap_or_else(|| DEFAULT_BACKEND.to_string())
}

/// Resolve the backend: an explicit override wins, else the persisted/default.
fn resolve_backend(config: &Config, override_url: Option<&str>) -> String {
    override_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| backend_base(config))
}

/// A started device flow: the code/URL to show the user plus the handle (and
/// resolved backend) to poll with.
pub struct DeviceStart {
    /// Code the user types at [`Self::verification_uri`].
    pub user_code: String,
    /// URL the user opens.
    pub verification_uri: String,
    /// Opaque handle passed back to [`device_poll`].
    pub poll_handle: String,
    /// Minimum seconds between polls.
    pub interval_secs: u64,
    /// Seconds until the flow expires.
    pub expires_in_secs: u64,
    /// The resolved backend base URL (echo back to [`device_poll`]).
    pub backend: String,
}

/// Begin a device flow against the (optionally overridden) backend.
pub async fn device_start(support_dir: &Path, backend_override: Option<&str>) -> Result<DeviceStart> {
    let config = load_config(support_dir)?;
    let backend = resolve_backend(&config, backend_override);
    let resp: DeviceStartResponse = reqwest::Client::new()
        .post(format!("{backend}/auth/device/start"))
        .json(&DeviceStartRequest::default())
        .send()
        .await
        .context("calling /auth/device/start")?
        .error_for_status()
        .context("/auth/device/start failed")?
        .json()
        .await
        .context("parsing device start response")?;
    Ok(DeviceStart {
        user_code: resp.user_code,
        verification_uri: resp.verification_uri,
        poll_handle: resp.poll_handle,
        interval_secs: resp.interval_secs,
        expires_in_secs: resp.expires_in_secs,
        backend,
    })
}

/// Outcome of one device-flow poll.
pub enum PollOutcome {
    /// Not authorized yet; keep polling.
    Pending,
    /// Polling too fast; back off then keep polling.
    SlowDown,
    /// The flow expired; restart.
    Expired,
    /// The user denied the request.
    Denied,
    /// Authorized; the session has been persisted.
    Authorized {
        /// GitHub login of the signed-in user.
        login: String,
        /// GitHub account id, if known.
        account_id: Option<String>,
    },
}

/// Poll a device flow once; on authorization, persist the session + backend.
pub async fn device_poll(
    support_dir: &Path,
    backend: &str,
    poll_handle: String,
) -> Result<PollOutcome> {
    let resp: DevicePollResponse = reqwest::Client::new()
        .post(format!("{backend}/auth/device/poll"))
        .json(&DevicePollRequest { poll_handle })
        .send()
        .await
        .context("calling /auth/device/poll")?
        .error_for_status()
        .context("/auth/device/poll failed")?
        .json()
        .await
        .context("parsing device poll response")?;
    match resp.status {
        DevicePollStatus::Pending => Ok(PollOutcome::Pending),
        DevicePollStatus::SlowDown => Ok(PollOutcome::SlowDown),
        DevicePollStatus::Expired => Ok(PollOutcome::Expired),
        DevicePollStatus::Denied => Ok(PollOutcome::Denied),
        DevicePollStatus::Authorized => {
            let cred = resp
                .credential
                .ok_or_else(|| anyhow!("backend reported authorized without a credential"))?;
            let mut config = load_config(support_dir)?;
            config.set_account_session(
                &cred.token,
                &cred.refresh_token,
                &cred.login,
                cred.account_id.clone(),
            );
            config.set_account_backend(backend);
            save_config(support_dir, &config)?;
            Ok(PollOutcome::Authorized {
                login: cred.login,
                account_id: cred.account_id,
            })
        }
    }
}

/// The signed-in identity.
pub struct AccountUser {
    /// GitHub login.
    pub login: String,
    /// GitHub account id, if known.
    pub account_id: Option<String>,
}

/// Return the signed-in user (verified against `/v1/me`), or `None` when not
/// signed in.
pub async fn current_user(support_dir: &Path) -> Result<Option<AccountUser>> {
    let mut config = load_config(support_dir)?;
    if config.account_token().is_none() {
        return Ok(None);
    }
    let backend = backend_base(&config);
    let token = valid_token(support_dir, &mut config, &backend).await?;
    let me: MeResponse = reqwest::Client::new()
        .get(format!("{backend}/v1/me"))
        .bearer_auth(&token)
        .send()
        .await
        .context("calling /v1/me")?
        .error_for_status()
        .context("/v1/me failed")?
        .json()
        .await
        .context("parsing /v1/me")?;
    Ok(Some(AccountUser {
        login: me.login,
        account_id: me.account_id,
    }))
}

/// Revoke the refresh token (best effort) and clear the local session.
pub async fn logout(support_dir: &Path) -> Result<()> {
    let mut config = load_config(support_dir)?;
    let backend = backend_base(&config);
    if let Some(refresh_token) = config.account_refresh_token() {
        let _ = reqwest::Client::new()
            .post(format!("{backend}/auth/logout"))
            .json(&LogoutRequest {
                refresh_token: refresh_token.to_string(),
            })
            .send()
            .await;
    }
    config.clear_account();
    save_config(support_dir, &config)?;
    Ok(())
}

/// Fetch the account's services from the backend (refreshing the token if needed).
pub async fn services(support_dir: &Path) -> Result<Vec<ServiceEntry>> {
    let mut config = load_config(support_dir)?;
    let backend = backend_base(&config);
    let token = valid_token(support_dir, &mut config, &backend).await?;
    let body: pocket_codex_account_proto::http::ServicesResponse = reqwest::Client::new()
        .get(format!("{backend}/v1/services"))
        .bearer_auth(&token)
        .send()
        .await
        .context("calling /v1/services")?
        .error_for_status()
        .context("/v1/services failed")?
        .json()
        .await
        .context("parsing /v1/services")?;
    Ok(body.services)
}

/// Return a currently-valid token, refreshing when missing/near-expiry.
async fn valid_token(support_dir: &Path, config: &mut Config, backend: &str) -> Result<String> {
    if let Some(token) = config.account_token() {
        if jwt_exp(token).is_none_or(|exp| exp > unix_now() + 60) {
            return Ok(token.to_string());
        }
    }
    let refresh_token = config
        .account_refresh_token()
        .ok_or_else(|| anyhow!("not signed in"))?
        .to_string();
    let resp = reqwest::Client::new()
        .post(format!("{backend}/auth/refresh"))
        .json(&RefreshRequest { refresh_token })
        .send()
        .await
        .context("calling /auth/refresh")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("session expired; sign in again");
    }
    let body: RefreshResponse = resp
        .error_for_status()
        .context("/auth/refresh failed")?
        .json()
        .await
        .context("parsing refresh response")?;
    let cred = body.credential;
    config.set_account_session(
        &cred.token,
        &cred.refresh_token,
        &cred.login,
        cred.account_id.clone(),
    );
    save_config(support_dir, config)?;
    Ok(cred.token)
}

/// Decode a JWT's `exp` (unix seconds) without verifying the signature.
fn jwt_exp(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("exp")?.as_i64()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_base_defaults_then_uses_config() {
        let mut config = Config::default();
        assert_eq!(backend_base(&config), DEFAULT_BACKEND);
        config.set_account_backend("https://cfg.example");
        assert_eq!(backend_base(&config), "https://cfg.example");
    }

    #[test]
    fn resolve_backend_override_wins() {
        let config = Config::default();
        assert_eq!(
            resolve_backend(&config, Some("https://flag.example")),
            "https://flag.example"
        );
        assert_eq!(resolve_backend(&config, None), DEFAULT_BACKEND);
    }

    #[test]
    fn jwt_exp_reads_exp() {
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"exp\":1700000000}");
        assert_eq!(jwt_exp(&format!("h.{payload}.s")), Some(1_700_000_000));
        assert_eq!(jwt_exp("nope"), None);
    }
}
