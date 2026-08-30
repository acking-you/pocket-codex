//! Hosted-account engine: GitHub login, session persistence and refresh,
//! identity, the per-account services listing, and the `/v1/relay` credential
//! handoff — all over the backend HTTP API.
//!
//! The app never sees the relay's ADMINISTRATOR key. It holds a backend-issued
//! session token (a JWT in the 0600 `config.toml`), the opaque refresh token,
//! and a short-lived relay credential the relay confines to this account's
//! namespace. After [`relay_credential`] the backend is off the path entirely —
//! see [`crate::engine::transport`].
//!
//! Pure async logic (no flutter_rust_bridge); the `api` layer drives it on the
//! engine runtime.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_account_proto::{
    http::{
        session_token_exp, DevicePollRequest, DevicePollResponse, DevicePollStatus,
        DeviceStartRequest, DeviceStartResponse, LogoutRequest, MeResponse, RefreshRequest,
        RefreshResponse, RelayCredentialResponse, ServiceEntry, WebExchangeRequest,
        WebExchangeResponse, WebStartRequest, WebStartResponse,
    },
    pkce,
};
use pocket_codex_core::{config::Config, service::sanitize_component};
use pocket_codex_pb::RelaySession;

use crate::engine::config::{load_config, save_config};

/// Compile-time default backend host, overridable at build time via the
/// `POCKET_CODEX_BACKEND_HOST` env var (the release pipeline injects the repo's
/// configured server). An empty/unset value falls back to the bundled default.
const DEFAULT_BACKEND_HOST: Option<&str> = option_env!("POCKET_CODEX_BACKEND_HOST");

/// The compile-time default backend API base URL — `https://<host>:8443`, where
/// `<host>` is the build-time [`DEFAULT_BACKEND_HOST`] or the bundled fallback.
pub fn default_backend() -> String {
    let host = match DEFAULT_BACKEND_HOST {
        Some(host) if !host.is_empty() => host,
        _ => "lb7666.top",
    };
    format!("https://{host}:8443")
}

/// The persisted backend base URL, or the built-in default.
pub fn backend_base(config: &Config) -> String {
    config
        .account_backend()
        .map(ToString::to_string)
        .unwrap_or_else(default_backend)
}

/// Resolve the backend: an explicit override wins, else the persisted/default.
/// An override must be `https://` so the bearer JWT + refresh token are never
/// sent in cleartext to a mistyped or hostile endpoint (the default is https).
fn resolve_backend(config: &Config, override_url: Option<&str>) -> Result<String> {
    match override_url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(url) => {
            if !url.starts_with("https://") {
                bail!(
                    "backend URL must start with https:// (got `{url}`); the session token must \
                     not be sent in cleartext"
                );
            }
            Ok(url.to_string())
        },
        None => Ok(backend_base(config)),
    }
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
pub async fn device_start(
    support_dir: &Path,
    backend_override: Option<&str>,
) -> Result<DeviceStart> {
    let config = load_config(support_dir)?;
    let backend = resolve_backend(&config, backend_override)?;
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
        .json(&DevicePollRequest {
            poll_handle,
        })
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
        },
    }
}

/// A started web (authorization-code) login: the browser URL to open plus the
/// per-flow secrets the caller carries to [`web_login_exchange`]. The
/// `code_verifier` and `state` stay on-device (Dart holds them across the
/// browser round-trip); only the verifier's challenge ever reaches the backend.
pub struct WebLoginStart {
    /// GitHub authorization URL to open in a browser.
    pub authorize_url: String,
    /// CSRF state the caller must match against the final redirect's `state`.
    pub state: String,
    /// PKCE verifier the caller passes back to [`web_login_exchange`].
    pub code_verifier: String,
    /// The resolved backend base URL (echo back to [`web_login_exchange`]).
    pub backend: String,
}

/// Begin a web (authorization-code / browser-redirect) login. `redirect_uri` is
/// the platform-specific callback the browser is sent back to at the end (the
/// app's custom scheme on mobile, a loopback URL on desktop); the caller drives
/// the browser, then redeems the returned exchange code via
/// [`web_login_exchange`].
pub async fn web_login_start(
    support_dir: &Path,
    redirect_uri: &str,
    backend_override: Option<&str>,
) -> Result<WebLoginStart> {
    let config = load_config(support_dir)?;
    let backend = resolve_backend(&config, backend_override)?;
    let code_verifier = pkce::gen_verifier();
    let state = pkce::gen_state();
    let resp: WebStartResponse = reqwest::Client::new()
        .post(format!("{backend}/auth/web/start"))
        .json(&WebStartRequest {
            redirect_uri: redirect_uri.to_string(),
            state: state.clone(),
            code_challenge: pkce::challenge(&code_verifier),
            device_label: None,
        })
        .send()
        .await
        .context("calling /auth/web/start")?
        .error_for_status()
        .context("/auth/web/start failed")?
        .json()
        .await
        .context("parsing web start response")?;
    Ok(WebLoginStart {
        authorize_url: resp.authorize_url,
        state,
        code_verifier,
        backend,
    })
}

/// Redeem the one-time exchange code (with its PKCE verifier) returned by the
/// browser redirect for a session, persisting it exactly like the device flow.
pub async fn web_login_exchange(
    support_dir: &Path,
    backend: &str,
    exchange_code: String,
    code_verifier: String,
) -> Result<PollOutcome> {
    let resp: WebExchangeResponse = reqwest::Client::new()
        .post(format!("{backend}/auth/web/exchange"))
        .json(&WebExchangeRequest {
            exchange_code,
            code_verifier,
        })
        .send()
        .await
        .context("calling /auth/web/exchange")?
        .error_for_status()
        .context("/auth/web/exchange failed")?
        .json()
        .await
        .context("parsing web exchange response")?;
    let cred = resp.credential;
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
    // The next sign-in must not inherit this account's namespace — a cached
    // credential would have it registering into someone else's.
    forget_relay_credential().await;
    Ok(())
}

/// Fetch the account's services from the backend (refreshing the token if
/// needed).
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

/// Deregister one of the account's services from the relay (best-effort).
/// `kind` is `"app"` or `"api"`. The backend derives the relay key from the
/// verified token, so this can only ever drop the caller's own keys; a client
/// still hosting the service will reconnect and re-register shortly after.
pub async fn deregister_service(
    support_dir: &Path,
    device: &str,
    kind: &str,
    name: &str,
) -> Result<()> {
    let mut config = load_config(support_dir)?;
    let backend = backend_base(&config);
    let token = valid_token(support_dir, &mut config, &backend).await?;
    // Sanitize the user-chosen segments to the exact key the backend derives, so
    // a name with `/`, `#`, or `?` can't break the URL path / target a wrong key
    // (the relay key was registered through the same sanitizer).
    let device = sanitize_component(device);
    let name = sanitize_component(name);
    reqwest::Client::new()
        .delete(format!("{backend}/v1/services/{device}/{kind}/{name}"))
        .bearer_auth(&token)
        .send()
        .await
        .context("calling DELETE /v1/services")?
        .error_for_status()
        .context("/v1/services deregister failed")?;
    Ok(())
}

/// Return a currently-valid token, refreshing when missing/near-expiry.
async fn valid_token(support_dir: &Path, config: &mut Config, backend: &str) -> Result<String> {
    // Fast path: the in-hand token is comfortably valid. An unparsable / exp-less
    // token does NOT count as valid here — it falls through to a refresh.
    if let Some(token) = config.account_token() {
        if session_token_exp(token).is_some_and(|exp| exp > unix_now() + 60) {
            return Ok(token.to_string());
        }
    }
    // Refresh is needed. Serialize it process-wide so concurrent callers (the
    // relay credential fetch, plus FRB queries) don't each spend the
    // single-use, rotating refresh token and lost-update each other's writes.
    let _guard = refresh_lock().lock().await;
    // Re-read from disk and re-check: another waiter may have refreshed while we
    // were queued, in which case we reuse its freshly-persisted token.
    *config = load_config(support_dir)?;
    if let Some(token) = config.account_token() {
        if session_token_exp(token).is_some_and(|exp| exp > unix_now() + 60) {
            return Ok(token.to_string());
        }
    }
    let refresh_token = config
        .account_refresh_token()
        .ok_or_else(|| anyhow!("not signed in"))?
        .to_string();
    let resp = reqwest::Client::new()
        .post(format!("{backend}/auth/refresh"))
        .json(&RefreshRequest {
            refresh_token,
        })
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

/// Process-global lock serializing token refreshes, so overlapping callers
/// don't each spend the rotating refresh token (401-ing the losers) or
/// lost-update each other's persisted credential.
fn refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: once_cell::sync::OnceCell<tokio::sync::Mutex<()>> =
        once_cell::sync::OnceCell::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// This account's relay credential, from cache when it is still good.
///
/// The last thing the app needs the backend for: everything after it —
/// register, subscribe, every byte — is app↔relay.
///
/// Cached because the app asks for this on every subscribe, probe, and key
/// derivation, and a round trip per call would put the backend back on a hot
/// path this whole design exists to take it off.
///
/// # The cache is keyed by WHO it belongs to
///
/// A process-global "current credential" would survive a change of account. The
/// dangerous path is not logout — that clears it explicitly — but a session
/// that EXPIRED and was replaced by signing in as someone else: `config.toml`
/// changes while the cache does not, so the new user would keep publishing into
/// the previous account's namespace. Keying on `(backend, account id)` means a
/// different signed-in identity simply misses the cache.
pub async fn relay_credential(support_dir: &Path) -> Result<RelayCredentialResponse> {
    let config = load_config(support_dir)?;
    let owner = CacheOwner::of(&config);
    // Held across the fetch, which is what makes a burst of first-time callers
    // cost one request rather than one each — the same reason [`refresh_lock`]
    // exists for the session token.
    let mut cache = relay_cache().lock().await;
    if let Some((cached_owner, cached)) = cache.as_ref() {
        // Same identity, and still enough life left that a caller can open a
        // tunnel with what we hand back.
        if *cached_owner == owner
            && cached.expires_at > (unix_now() + RELAY_CACHE_MARGIN_SECS).max(0) as u64
        {
            return Ok(cached.clone());
        }
    }
    let fetched = fetch_relay_credential(support_dir).await?;
    *cache = Some((owner, fetched.clone()));
    Ok(fetched)
}

/// Discard the cached relay credential, so the next call re-fetches.
pub async fn forget_relay_credential() {
    *relay_cache().lock().await = None;
}

/// Who a cached credential belongs to.
///
/// The backend too, not just the account: the same GitHub user against a
/// different backend is a different relay and a different namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheOwner {
    backend: String,
    account_id: Option<String>,
    login: Option<String>,
}

impl CacheOwner {
    fn of(config: &Config) -> Self {
        Self {
            backend: backend_base(config),
            account_id: config.account_id().map(ToString::to_string),
            // Carried alongside the id because `account_id` is optional in the
            // credential the backend issues; two accounts must never compare
            // equal just because neither reported one.
            login: config.account_login().map(ToString::to_string),
        }
    }
}

/// Treat a credential with less than this remaining as due for renewal rather
/// than handing it out.
const RELAY_CACHE_MARGIN_SECS: i64 = 5 * 60;

type RelayCache = tokio::sync::Mutex<Option<(CacheOwner, RelayCredentialResponse)>>;

fn relay_cache() -> &'static RelayCache {
    static CACHE: OnceCell<RelayCache> = OnceCell::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// Ask the backend for a relay credential, bypassing the cache.
async fn fetch_relay_credential(support_dir: &Path) -> Result<RelayCredentialResponse> {
    let mut config = load_config(support_dir)?;
    let backend = backend_base(&config);
    let token = valid_token(support_dir, &mut config, &backend).await?;
    let resp = reqwest::Client::new()
        .get(format!("{backend}/v1/relay"))
        .bearer_auth(&token)
        .send()
        .await
        .context("calling /v1/relay")?;
    // A backend without `/v1/relay` predates direct relay access and still
    // expects clients to tunnel through it. Say so, because the fix is a server
    // upgrade rather than anything the user can do here.
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        bail!(
            "this backend does not support direct relay access yet; update the Pocket-Codex server"
        );
    }
    resp.error_for_status()
        .context("/v1/relay failed")?
        .json()
        .await
        .context("parsing /v1/relay")
}

/// This account's relay session, plus the expiry to schedule a refresh against.
///
/// The pair every account-mode tunnel starts from. Callers that hold a tunnel
/// open should pass `expires_at` to [`pocket_codex_pb::keep_credential_alive`]:
/// the relay cancels a credential's tunnels when it lapses, so a long-lived
/// host must keep renewing or stop serving at the TTL.
pub async fn relay_session(support_dir: &Path) -> Result<(RelaySession, u64)> {
    let relay = relay_credential(support_dir).await?;
    Ok((RelaySession::new(relay.relay_addr, relay.credential), relay.expires_at))
}

/// Start the background task that keeps this account's credential renewed.
///
/// Call it alongside anything that holds a tunnel open: the relay cancels a
/// credential's tunnels when it lapses, so a host that registered once and
/// never asked again would stop serving at the TTL with nothing having gone
/// wrong. Idempotent — one refresher per process is enough, since devices on an
/// account share one credential.
pub fn start_credential_refresh(support_dir: &Path, expires_at: u64) {
    static STARTED: OnceCell<()> = OnceCell::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let support = support_dir.to_path_buf();
    pocket_codex_pb::keep_credential_alive(expires_at, move || {
        let support = support.clone();
        async move {
            // Dropping the cache first is what makes this a REFRESH: the backend
            // renews the same credential and returns the later expiry, and going
            // through the cached path would just hand back the value we are
            // trying to extend.
            forget_relay_credential().await;
            Ok(relay_credential(&support).await?.expires_at)
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_different_signed_in_account_is_a_different_cache_owner() {
        // The whole point of keying the cache: a session that EXPIRED and was
        // replaced by signing in as someone else changes `config.toml` but not the
        // cache, and an unkeyed cache would hand the new user the previous
        // account's credential — letting them publish into that namespace.
        let mut alice = Config::default();
        alice.set_account_session("tok-a", "ref-a", "alice", Some("1".to_string()));
        let mut bob = Config::default();
        bob.set_account_session("tok-b", "ref-b", "bob", Some("2".to_string()));
        assert_ne!(CacheOwner::of(&alice), CacheOwner::of(&bob));

        // A token ROTATION is the same account, so it must keep the cache: the
        // credential outlives any one session token, and re-minting on refresh
        // would change the namespace.
        let mut alice_refreshed = alice.clone();
        alice_refreshed.set_account_session("tok-a2", "ref-a2", "alice", Some("1".to_string()));
        assert_eq!(CacheOwner::of(&alice), CacheOwner::of(&alice_refreshed));
    }

    #[test]
    fn two_accounts_without_an_id_are_told_apart_by_login() {
        // `account_id` is optional in the issued credential. Comparing on it alone
        // would make every id-less account equal to every other.
        let mut one = Config::default();
        one.set_account_session("tok", "ref", "alice", None);
        let mut two = Config::default();
        two.set_account_session("tok", "ref", "bob", None);
        assert_ne!(CacheOwner::of(&one), CacheOwner::of(&two));
    }

    #[test]
    fn the_same_account_on_another_backend_is_a_different_owner() {
        // A different backend means a different relay and a different namespace,
        // so its credential is not interchangeable.
        let mut here = Config::default();
        here.set_account_session("tok", "ref", "alice", Some("1".to_string()));
        let mut there = here.clone();
        there.set_account_backend("https://other.example");
        assert_ne!(CacheOwner::of(&here), CacheOwner::of(&there));
    }

    #[test]
    fn backend_base_defaults_then_uses_config() {
        let mut config = Config::default();
        assert_eq!(backend_base(&config), default_backend());
        config.set_account_backend("https://cfg.example");
        assert_eq!(backend_base(&config), "https://cfg.example");
    }

    #[test]
    fn resolve_backend_override_wins_and_requires_https() {
        let config = Config::default();
        assert_eq!(
            resolve_backend(&config, Some("https://flag.example")).expect("https override"),
            "https://flag.example"
        );
        assert_eq!(resolve_backend(&config, None).expect("default backend"), default_backend());
        // An http override is rejected so the bearer token can't go out in cleartext.
        assert!(resolve_backend(&config, Some("http://insecure.example")).is_err());
    }
}
