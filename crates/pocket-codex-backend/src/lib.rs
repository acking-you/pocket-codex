//! Hosted Pocket-Codex backend.
//!
//! An identity and credential-vending service, and nothing more. It:
//! - serves the GitHub-device-flow HTTP API ([`api::router`]) plus the
//!   per-account `/v1/me` + `/v1/services` views;
//! - vends each account a short-lived pb-mapper credential over `/v1/relay`
//!   ([`credentials`]), minted under the administrator key this process alone
//!   holds;
//! - terminates TLS in-process (plain / cert files / ACME).
//!
//! # It is not on the data path
//!
//! Clients take that credential and `register`/`connect` against the relay
//! **directly**. The backend used to tunnel every byte through a broker on its
//! own port, which cost an extra hop and made backend availability a
//! prerequisite for two of a user's own devices to talk. Now a device needs the
//! backend once per credential lifetime and never again.
//!
//! HTTP serving is exposed as library items so integration tests can drive it
//! over plain TCP.

#![forbid(unsafe_code)]

pub mod config;

mod api;
pub mod credentials;
mod serve;
mod tls;

use std::sync::Arc;

pub use api::{router, AppState};
pub use config::{ServerConfig, TlsMode};
pub use credentials::Credentials;
use pocket_codex_auth::Auth;
use pocket_codex_store::Store;
use tokio::net::TcpListener;

/// Run the backend (the HTTP API over the configured TLS layer) until a fatal
/// error.
pub async fn run(cfg: ServerConfig) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let store = Store::connect(&cfg.database_url).await?;

    // Periodically purge expired device flows + refresh tokens so the tables
    // don't grow unbounded under device-flow churn / abandoned logins.
    let purge_store = store.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            if let Err(e) = purge_store.purge_expired(now).await {
                tracing::warn!(error = %e, "periodic purge_expired failed");
            }
        }
    });

    // The web (authorization-code) flow's OAuth callback the backend hands to
    // GitHub — derived from the configured public URL, and enabled only when a
    // client secret is also present (Auth gates on both).
    let web_callback_url = cfg
        .public_url
        .as_deref()
        .map(|u| format!("{}/auth/web/callback", u.trim_end_matches('/')));
    let auth = Arc::new(Auth::new(store, pocket_codex_auth::Config {
        github_client_id: cfg.github_client_id.clone(),
        github_client_secret: cfg.github_client_secret.clone(),
        github_scope: cfg.github_scope.clone(),
        jwt_secret: cfg.jwt_secret.clone(),
        jwt_ttl_secs: cfg.jwt_ttl_secs,
        refresh_ttl_secs: cfg.refresh_ttl_secs,
        web_callback_url,
    })?);
    tracing::info!(web_login = auth.web_enabled(), "auth flows configured");
    // Validated at boot even though the SDK takes the address as a string: a
    // typo'd relay_addr should fail the process here, not surface later as a
    // per-tunnel connect error on a backend that looked healthy.
    pocket_codex_pb::parse_relay_addr(&cfg.relay_addr)?;

    // The ADMINISTRATOR credential, which this process alone holds and never
    // hands out. Two uses, both administrative: minting the per-account
    // credentials clients do get, and acting on an account's whole relay view
    // (listing services across namespaces, retiring an abandoned registration).
    let relay = pocket_codex_pb::RelaySession::new(
        cfg.relay_addr.clone(),
        cfg.relay_credential()
            .ok_or_else(|| anyhow::anyhow!("relay_credential is required"))?,
    );

    let tls = tls::build_tls(&cfg)?;
    let http_listener = TcpListener::bind(&cfg.http_listen)
        .await
        .map_err(|e| anyhow::anyhow!("binding http {}: {e}", cfg.http_listen))?;

    let app = api::router(AppState {
        auth,
        credentials: credentials::Credentials::new(relay.clone()),
        relay,
    });
    tracing::info!(
        http = %cfg.http_listen,
        relay = %cfg.relay_addr,
        tls = ?cfg.tls_mode,
        "pocket-codex backend up"
    );

    serve::serve_http(http_listener, app, tls).await;
    Ok(())
}
