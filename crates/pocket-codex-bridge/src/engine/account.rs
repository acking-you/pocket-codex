//! Hosted-account engine: GitHub device-flow login, session persistence and
//! refresh, identity, and the per-account services listing — all over the
//! backend HTTP API. The app holds only the backend-issued session token (a
//! JWT, persisted in the same 0600 `config.toml` as the relay key) and the
//! opaque refresh token; it never sees the relay key.
//!
//! Pure async logic (no flutter_rust_bridge); the `api` layer drives it on the
//! engine runtime.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use pocket_codex_account_proto::http::{
    DevicePollRequest, DevicePollResponse, DevicePollStatus, DeviceStartRequest,
    DeviceStartResponse, LogoutRequest, MeResponse, RefreshRequest, RefreshResponse, ServiceEntry,
};
use pocket_codex_broker_client::{BrokerError, BrokerStream, Connector, TokenProvider};
use pocket_codex_core::config::Config;
use tokio::net::TcpStream;

use crate::engine::config::{load_config, save_config};

/// Compile-time default backend base URL.
pub const DEFAULT_BACKEND: &str = "https://lb7666.top:8443";
/// Default broker TLS port; the host comes from the backend URL.
const DEFAULT_BROKER_PORT: u16 = 7900;
/// Idle timeout applied to account-mode data bridges.
pub const ACCOUNT_DATA_IDLE: Duration = Duration::from_secs(1800);

/// The persisted backend base URL, or the built-in default.
pub fn backend_base(config: &Config) -> String {
    config
        .account_backend()
        .map(ToString::to_string)
        .unwrap_or_else(|| DEFAULT_BACKEND.to_string())
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
        if jwt_exp(token).is_some_and(|exp| exp > unix_now() + 60) {
            return Ok(token.to_string());
        }
    }
    // Refresh is needed. Serialize it process-wide so concurrent callers (the
    // broker opens a tunnel per stream, plus FRB queries) don't each spend the
    // single-use, rotating refresh token and lost-update each other's writes.
    let _guard = refresh_lock().lock().await;
    // Re-read from disk and re-check: another waiter may have refreshed while we
    // were queued, in which case we reuse its freshly-persisted token.
    *config = load_config(support_dir)?;
    if let Some(token) = config.account_token() {
        if jwt_exp(token).is_some_and(|exp| exp > unix_now() + 60) {
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

/// Derive the broker `host` + `port` from the backend URL.
pub fn broker_endpoint(backend: &str) -> Result<(String, u16)> {
    let url =
        reqwest::Url::parse(backend).with_context(|| format!("parsing backend url {backend}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("backend url {backend} has no host"))?
        .to_string();
    let port = std::env::var("POCKET_CODEX_BROKER_PORT")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(DEFAULT_BROKER_PORT);
    Ok((host, port))
}

/// Build a broker TLS connector + token provider for the configured backend, so
/// the app can open account-mode tunnels the same way the CLI does.
pub fn broker_transport(
    support_dir: &Path,
) -> Result<(Arc<dyn Connector>, Arc<dyn TokenProvider>)> {
    let config = load_config(support_dir)?;
    let backend = backend_base(&config);
    let (host, port) = broker_endpoint(&backend)?;
    let connector: Arc<dyn Connector> = Arc::new(BrokerTlsConnector::new(host, port)?);
    let tokens: Arc<dyn TokenProvider> = Arc::new(ConfigTokenProvider {
        support_dir: support_dir.to_path_buf(),
        backend,
    });
    Ok((connector, tokens))
}

/// Opens TLS broker tunnels to the backend, trusting the bundled webpki roots
/// (portable across desktop + mobile, no OS trust-store integration needed).
struct BrokerTlsConnector {
    host: String,
    addr: String,
    tls: tokio_rustls::TlsConnector,
}

impl BrokerTlsConnector {
    fn new(host: String, port: u16) -> Result<Self> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            addr: format!("{host}:{port}"),
            host,
            tls: tokio_rustls::TlsConnector::from(Arc::new(config)),
        })
    }
}

#[async_trait::async_trait]
impl Connector for BrokerTlsConnector {
    async fn connect(&self) -> std::result::Result<Box<dyn BrokerStream>, BrokerError> {
        let tcp = TcpStream::connect(&self.addr).await?;
        let server_name = rustls::pki_types::ServerName::try_from(self.host.clone())
            .map_err(|e| BrokerError::Token(format!("invalid broker host {}: {e}", self.host)))?;
        let tls = self
            .tls
            .connect(server_name, tcp)
            .await
            .map_err(BrokerError::Io)?;
        Ok(Box::new(tls))
    }
}

/// Supplies a valid token on every (re)connect, refreshing near expiry.
struct ConfigTokenProvider {
    support_dir: PathBuf,
    backend: String,
}

#[async_trait::async_trait]
impl TokenProvider for ConfigTokenProvider {
    async fn token(&self) -> std::result::Result<String, BrokerError> {
        let mut config =
            load_config(&self.support_dir).map_err(|e| BrokerError::Token(e.to_string()))?;
        valid_token(&self.support_dir, &mut config, &self.backend)
            .await
            .map_err(|e| BrokerError::Token(e.to_string()))
    }
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
    fn resolve_backend_override_wins_and_requires_https() {
        let config = Config::default();
        assert_eq!(
            resolve_backend(&config, Some("https://flag.example")).expect("https override"),
            "https://flag.example"
        );
        assert_eq!(resolve_backend(&config, None).expect("default backend"), DEFAULT_BACKEND);
        // An http override is rejected so the bearer token can't go out in cleartext.
        assert!(resolve_backend(&config, Some("http://insecure.example")).is_err());
    }

    #[test]
    fn jwt_exp_reads_exp() {
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"exp\":1700000000}");
        assert_eq!(jwt_exp(&format!("h.{payload}.s")), Some(1_700_000_000));
        assert_eq!(jwt_exp("nope"), None);
    }
}
