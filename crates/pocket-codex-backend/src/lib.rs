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

use std::{net::SocketAddr, sync::Arc};

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
    // An explicit key, or `None` to adopt the relay's machine-derived key
    // (matching `pb-mapper-server --use-machine-msg-header-key` on the same host).
    let configured_key = cfg
        .msg_header_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    pocket_codex_pb::set_msg_header_key(configured_key)
        .map_err(|e| anyhow::anyhow!("invalid msg_header_key: {e}"))?;

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

    let auth = Arc::new(Auth::new(store, pocket_codex_auth::Config {
        github_client_id: cfg.github_client_id.clone(),
        github_scope: cfg.github_scope.clone(),
        jwt_secret: cfg.jwt_secret.clone(),
        jwt_ttl_secs: cfg.jwt_ttl_secs,
        refresh_ttl_secs: cfg.refresh_ttl_secs,
    })?);
    let relay_addr: SocketAddr = cfg
        .relay_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("relay_addr `{}`: {e}", cfg.relay_addr))?;

    let verifier = Arc::new(AuthVerifier(auth.clone()));
    let broker = BrokerServer::new(verifier, cfg.relay_addr.clone(), cfg.data_idle());
    let tls = tls::build_tls(&cfg)?;

    let http_listener = TcpListener::bind(&cfg.http_listen)
        .await
        .map_err(|e| anyhow::anyhow!("binding http {}: {e}", cfg.http_listen))?;
    let broker_listener = TcpListener::bind(&cfg.broker_listen)
        .await
        .map_err(|e| anyhow::anyhow!("binding broker {}: {e}", cfg.broker_listen))?;

    let app = api::router(AppState {
        auth,
        relay_addr,
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
