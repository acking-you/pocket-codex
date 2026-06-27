//! Hosted-account client: GitHub device-flow login, token persistence and
//! refresh, and the TLS broker connector + token provider the account-mode
//! transport feeds to [`pocket_codex_broker_client`].
//!
//! The CLI never sees the relay key — it holds only the backend-issued session
//! token (a JWT, persisted in the same 0600 `config.toml` as the relay key) and
//! the opaque refresh token used to renew it.

use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use pocket_codex_account_proto::http::{
    DevicePollResponse, DevicePollStatus, DeviceStartRequest, DeviceStartResponse, LogoutRequest,
    MeResponse, RefreshRequest, RefreshResponse, ServiceEntry, ServicesResponse,
};
use pocket_codex_broker_client::{BrokerError, BrokerStream, Connector, TokenProvider};
use pocket_codex_core::{
    config::{Config, Mode},
    service::{default_device_id, sanitize_component, ServiceKind, DEFAULT_SERVICE_NAME},
};
use tokio::net::TcpStream;

use crate::commands::ui;

/// Compile-time default backend base URL.
pub(crate) const DEFAULT_BACKEND: &str = "https://lb7666.top:8443";
/// Default broker TLS port; the host is taken from the backend URL.
const DEFAULT_BROKER_PORT: u16 = 7900;

/// Resolve the backend base URL: `--backend` > config > `$POCKET_CODEX_BACKEND`
/// > the compile-time default.
pub(crate) fn backend_base(flag: Option<&str>, config: &Config) -> String {
    flag.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| config.account_backend().map(ToString::to_string))
        .or_else(|| {
            std::env::var("POCKET_CODEX_BACKEND")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_BACKEND.to_string())
}

/// Derive the broker `host` + `port` from the backend URL (`$POCKET_CODEX_BROKER_PORT`
/// overrides the default port).
pub(crate) fn broker_endpoint(backend: &str) -> Result<(String, u16)> {
    let url = url::Url::parse(backend).with_context(|| format!("parsing backend url `{backend}`"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("backend url `{backend}` has no host"))?
        .to_string();
    let port = std::env::var("POCKET_CODEX_BROKER_PORT")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(DEFAULT_BROKER_PORT);
    Ok((host, port))
}

/// `pocket-codex login`: run the GitHub device flow against the backend and
/// persist the issued session.
pub(crate) async fn login(backend_flag: Option<&str>) -> Result<()> {
    let mut config = Config::load()?;
    let base = backend_base(backend_flag, &config);
    let client = reqwest::Client::new();

    let start: DeviceStartResponse = client
        .post(format!("{base}/auth/device/start"))
        .json(&DeviceStartRequest {
            device_label: Some(default_device_id()),
        })
        .send()
        .await
        .context("calling /auth/device/start")?
        .error_for_status()
        .context("/auth/device/start failed")?
        .json()
        .await
        .context("parsing device start response")?;

    ui::headline(ui::Tone::Action, "sign in with GitHub");
    ui::field("code", &start.user_code);
    ui::field("url", &start.verification_uri);
    ui::code(&format!(
        "open {} and enter {}",
        start.verification_uri, start.user_code
    ));

    let interval = Duration::from_secs(start.interval_secs.max(1));
    loop {
        tokio::time::sleep(interval).await;
        let poll: DevicePollResponse = client
            .post(format!("{base}/auth/device/poll"))
            .json(&pocket_codex_account_proto::http::DevicePollRequest {
                poll_handle: start.poll_handle.clone(),
            })
            .send()
            .await
            .context("calling /auth/device/poll")?
            .error_for_status()
            .context("/auth/device/poll failed")?
            .json()
            .await
            .context("parsing device poll response")?;
        match poll.status {
            DevicePollStatus::Pending => continue,
            DevicePollStatus::SlowDown => tokio::time::sleep(interval).await,
            DevicePollStatus::Authorized => {
                let cred = poll
                    .credential
                    .ok_or_else(|| anyhow!("backend reported authorized without a credential"))?;
                if backend_flag.is_some() {
                    config.set_account_backend(&base);
                }
                config.set_account_session(
                    &cred.token,
                    &cred.refresh_token,
                    &cred.login,
                    cred.account_id.clone(),
                );
                config.save()?;
                ui::headline(ui::Tone::Ok, "signed in");
                ui::field("login", &cred.login);
                return Ok(());
            }
            DevicePollStatus::Expired => bail!("device code expired; run `pocket-codex login` again"),
            DevicePollStatus::Denied => bail!("access denied on GitHub"),
        }
    }
}

/// `pocket-codex logout`: revoke the refresh token (best effort) and clear the
/// local session.
pub(crate) async fn logout() -> Result<()> {
    let mut config = Config::load()?;
    let base = backend_base(None, &config);
    if let Some(refresh_token) = config.account_refresh_token() {
        let _ = reqwest::Client::new()
            .post(format!("{base}/auth/logout"))
            .json(&LogoutRequest {
                refresh_token: refresh_token.to_string(),
            })
            .send()
            .await;
    }
    config.clear_account();
    config.save()?;
    ui::headline(ui::Tone::Ok, "signed out");
    Ok(())
}

/// `pocket-codex account status`: show the signed-in identity (verified against
/// the backend) or the current self-host/unconfigured state.
pub(crate) async fn status() -> Result<()> {
    let mut config = Config::load()?;
    match config.account_mode() {
        Mode::Account => {
            let base = backend_base(None, &config);
            let token = valid_token(&mut config, &base).await?;
            let me: MeResponse = reqwest::Client::new()
                .get(format!("{base}/v1/me"))
                .bearer_auth(&token)
                .send()
                .await
                .context("calling /v1/me")?
                .error_for_status()
                .context("/v1/me failed")?
                .json()
                .await
                .context("parsing /v1/me")?;
            ui::headline(ui::Tone::Ok, "signed in");
            ui::field("login", &me.login);
            if let Some(id) = me.account_id {
                ui::field("account", &id);
            }
            ui::field("backend", &base);
        }
        Mode::SelfHost => {
            ui::headline(ui::Tone::Muted, "self-hosted mode");
            if let Some(relay) = config.relay() {
                ui::field("relay", relay);
            }
        }
        Mode::Unconfigured => {
            ui::headline(ui::Tone::Muted, "not configured");
            ui::code("pocket-codex login");
        }
    }
    Ok(())
}

/// Fetch the account's services from the backend, refreshing the token if needed.
pub(crate) async fn fetch_services(config: &mut Config, base: &str) -> Result<Vec<ServiceEntry>> {
    let token = valid_token(config, base).await?;
    let body: ServicesResponse = reqwest::Client::new()
        .get(format!("{base}/v1/services"))
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

/// Resolve the target `(device, name)` for a `kind` in account mode: an explicit
/// `--device` wins; otherwise discover the account's services of that kind and
/// auto-pick a single one, asking to disambiguate when there is more than one.
pub(crate) async fn resolve_target(
    config: &mut Config,
    backend: &str,
    kind: ServiceKind,
    device: Option<&str>,
    name: Option<&str>,
) -> Result<(String, String)> {
    if let Some(device) = device {
        let name = name
            .map(sanitize_component)
            .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_string());
        return Ok((sanitize_component(device), name));
    }
    let mut matches: Vec<_> = fetch_services(config, backend)
        .await?
        .into_iter()
        .filter(|s| s.kind == kind)
        .collect();
    let label = kind.as_key_segment();
    match matches.len() {
        0 => bail!("no {label} services in your account; run the matching serve on the host first"),
        1 => {
            let m = matches.remove(0);
            Ok((m.device, m.name))
        }
        _ => {
            let names: Vec<String> = matches
                .iter()
                .map(|s| format!("{}/{}", s.device, s.name))
                .collect();
            bail!(
                "multiple {label} services; pick one with --device <device> [--name <name>]: {}",
                names.join(", ")
            )
        }
    }
}

/// Return a currently-valid session token, refreshing it when it is missing,
/// unparsable, or within a minute of expiry.
async fn valid_token(config: &mut Config, base: &str) -> Result<String> {
    if let Some(token) = config.account_token() {
        if jwt_exp(token).is_none_or(|exp| exp > unix_now() + 60) {
            return Ok(token.to_string());
        }
    }
    refresh_session(config, base).await
}

/// Exchange the refresh token for a new session, persisting the rotation.
async fn refresh_session(config: &mut Config, base: &str) -> Result<String> {
    let refresh_token = config
        .account_refresh_token()
        .ok_or_else(|| anyhow!("not signed in; run `pocket-codex login`"))?
        .to_string();
    let resp = reqwest::Client::new()
        .post(format!("{base}/auth/refresh"))
        .json(&RefreshRequest { refresh_token })
        .send()
        .await
        .context("calling /auth/refresh")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("session expired; run `pocket-codex login` again");
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
    config.save()?;
    Ok(cred.token)
}

/// Decode a JWT's `exp` claim (unix seconds) without verifying the signature.
fn jwt_exp(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("exp")?.as_i64()
}

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A stable per-process id so a reconnect deterministically takes over the prior
/// register session rather than racing it.
pub(crate) fn client_instance_id() -> String {
    format!("cli-{}", std::process::id())
}

/// Opens TLS broker tunnels to the backend (host from the backend URL, default
/// broker port), trusting the OS certificate store.
pub(crate) struct BrokerTlsConnector {
    host: String,
    addr: String,
    tls: tokio_rustls::TlsConnector,
}

impl BrokerTlsConnector {
    /// Build a connector for `host:port`, loading the native root store.
    pub(crate) fn new(host: String, port: u16) -> Result<Self> {
        let mut roots = rustls::RootCertStore::empty();
        let loaded = rustls_native_certs::load_native_certs();
        for cert in loaded.certs {
            let _ = roots.add(cert);
        }
        anyhow::ensure!(
            !roots.is_empty(),
            "no trusted root certificates found on this system"
        );
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
            .map_err(|e| BrokerError::Token(format!("invalid broker host `{}`: {e}", self.host)))?;
        let tls = self
            .tls
            .connect(server_name, tcp)
            .await
            .map_err(BrokerError::Io)?;
        Ok(Box::new(tls))
    }
}

/// Supplies the broker tunnels a valid token on every (re)connect, refreshing
/// when the stored JWT is within a minute of expiry.
pub(crate) struct ConfigTokenProvider {
    base: String,
}

impl ConfigTokenProvider {
    /// Build a provider against the given backend base URL.
    pub(crate) fn new(base: String) -> Self {
        Self { base }
    }
}

#[async_trait::async_trait]
impl TokenProvider for ConfigTokenProvider {
    async fn token(&self) -> std::result::Result<String, BrokerError> {
        let mut config = Config::load().map_err(|e| BrokerError::Token(e.to_string()))?;
        valid_token(&mut config, &self.base)
            .await
            .map_err(|e| BrokerError::Token(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_base_precedence_flag_over_default() {
        let config = Config::default();
        assert_eq!(
            backend_base(Some("https://flag.example"), &config),
            "https://flag.example"
        );
        // No flag, no config, no env → default.
        assert_eq!(backend_base(None, &config), DEFAULT_BACKEND);
    }

    #[test]
    fn backend_base_uses_config_when_no_flag() {
        let mut config = Config::default();
        config.set_account_backend("https://cfg.example");
        assert_eq!(backend_base(None, &config), "https://cfg.example");
        // A flag still wins over config.
        assert_eq!(backend_base(Some("https://flag"), &config), "https://flag");
    }

    #[test]
    fn broker_endpoint_takes_host_from_backend_url() {
        let (host, port) = broker_endpoint("https://lb7666.top:8443").expect("endpoint");
        assert_eq!(host, "lb7666.top");
        assert_eq!(port, DEFAULT_BROKER_PORT);
        assert!(broker_endpoint("not a url").is_err());
    }

    #[test]
    fn jwt_exp_reads_exp_claim() {
        // header.payload.sig with payload {"exp": 1700000000}
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"exp\":1700000000}");
        let token = format!("h.{payload}.s");
        assert_eq!(jwt_exp(&token), Some(1_700_000_000));
        assert_eq!(jwt_exp("not-a-jwt"), None);
    }
}
