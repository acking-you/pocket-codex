//! Hosted Pocket-Codex backend.
//!
//! One self-contained service that:
//! - serves the GitHub-device-flow HTTP API ([`api::router`]) and the
//!   per-account `/v1/me` + `/v1/services` views;
//! - runs the broker ([`pocket_codex_broker_server`]) that bridges
//!   authenticated client tunnels to a loopback pb-mapper relay holding the
//!   real `MSG_HEADER_KEY`, namespacing every key per user;
//! - terminates TLS for both in-process (plain / cert files / ACME).
//!
//! The HTTP and broker logic are exposed as library items so they can be driven
//! directly from integration tests over plain TCP.

#![forbid(unsafe_code)]

pub mod config;

mod api;
mod serve;
mod tls;

use std::sync::Arc;

pub use api::{router, AppState};
pub use config::{ServerConfig, TlsMode};
use pocket_codex_auth::Auth;
use pocket_codex_broker_server::{BrokerServer, TokenVerifier};
use pocket_codex_store::Store;
use tokio::net::TcpListener;

/// Adapts [`Auth`]'s stateless JWT verification to the broker's
/// [`TokenVerifier`], so the broker never touches the database on the hot path.
pub struct AuthVerifier(pub Arc<Auth>);

impl TokenVerifier for AuthVerifier {
    fn verify(&self, token: &str) -> Option<String> {
        self.0.verify(token).ok().map(|claims| claims.sub)
    }
}

/// Run the backend (HTTP API + broker over a shared TLS layer) until a fatal
/// error or a serving task aborts.
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

    // The relay credential this process authenticates with. The backend is the
    // ONE component that holds it: clients present a session JWT to the broker
    // and never a relay credential, so per-account isolation is enforced here
    // rather than trusted to clients. It used to be installed process-globally
    // via `set_msg_header_key`; it is now a value threaded to the broker.
    let relay = pocket_codex_pb::RelaySession::new(
        cfg.relay_addr.clone(),
        cfg.relay_credential()
            .ok_or_else(|| anyhow::anyhow!("relay_credential is required"))?,
    );

    let verifier = Arc::new(AuthVerifier(auth.clone()));
    let broker = BrokerServer::new(verifier, relay.clone(), cfg.data_idle());
    let tls = tls::build_tls(&cfg)?;

    let http_listener = TcpListener::bind(&cfg.http_listen)
        .await
        .map_err(|e| anyhow::anyhow!("binding http {}: {e}", cfg.http_listen))?;
    let broker_listener = TcpListener::bind(&cfg.broker_listen)
        .await
        .map_err(|e| anyhow::anyhow!("binding broker {}: {e}", cfg.broker_listen))?;

    let app = api::router(AppState {
        auth,
        relay,
        broker: broker.clone(),
    });
    tracing::info!(
        http = %cfg.http_listen,
        broker = %cfg.broker_listen,
        tls = ?cfg.tls_mode,
        "pocket-codex backend up"
    );

    let http = tokio::spawn(serve::serve_http(http_listener, app, tls.clone()));
    let broker = tokio::spawn(serve::serve_broker(broker_listener, broker, tls));
    tokio::try_join!(http, broker)?;
    Ok(())
}
