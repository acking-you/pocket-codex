//! Layered backend configuration (a 0600 TOML file + `PCX_`-prefixed env).

use serde::Deserialize;

/// How the backend terminates TLS for the HTTP API and the broker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// No TLS — plain TCP (local testing, or behind a TLS-terminating proxy).
    #[default]
    Plain,
    /// Static certificate + key PEM files (e.g. Let's Encrypt via certbot).
    Files,
}

/// The backend's full configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// The real 32-byte pb-mapper `MSG_HEADER_KEY`. Never leaves the backend.
    pub msg_header_key: String,
    /// HS256 secret used to sign session JWTs.
    pub jwt_secret: String,
    /// GitHub OAuth app client id (Device Flow enabled).
    pub github_client_id: String,
    /// OAuth scope requested at login.
    #[serde(default = "default_scope")]
    pub github_scope: String,
    /// sqlx SQLite URL (e.g. `sqlite://pocket-codex.db`).
    #[serde(default = "default_database_url")]
    pub database_url: String,
    /// Loopback address of the pb-mapper relay.
    #[serde(default = "default_relay_addr")]
    pub relay_addr: String,
    /// Listen address for the HTTP API.
    #[serde(default = "default_http_listen")]
    pub http_listen: String,
    /// Listen address for the broker tunnel.
    #[serde(default = "default_broker_listen")]
    pub broker_listen: String,
    /// Session (JWT) lifetime in seconds.
    #[serde(default = "default_jwt_ttl")]
    pub jwt_ttl_secs: i64,
    /// Refresh-token lifetime in seconds.
    #[serde(default = "default_refresh_ttl")]
    pub refresh_ttl_secs: i64,
    /// Idle timeout for a data bridge, in seconds.
    #[serde(default = "default_data_idle")]
    pub data_idle_secs: u64,
    /// TLS termination mode.
    #[serde(default)]
    pub tls_mode: TlsMode,
    /// `tls_mode = "files"`: certificate chain PEM path.
    #[serde(default)]
    pub tls_cert: Option<String>,
    /// `tls_mode = "files"`: private key PEM path.
    #[serde(default)]
    pub tls_key: Option<String>,
}

impl ServerConfig {
    /// Load config from the TOML file at `$POCKET_CODEX_BACKEND_CONFIG` (default
    /// `backend.toml`, optional) layered under `PCX_`-prefixed env vars.
    pub fn load() -> anyhow::Result<Self> {
        use figment::{
            providers::{Env, Format, Toml},
            Figment,
        };
        let path = std::env::var("POCKET_CODEX_BACKEND_CONFIG")
            .unwrap_or_else(|_| "backend.toml".to_string());
        let config = Figment::new()
            .merge(Toml::file(path))
            .merge(Env::prefixed("PCX_"))
            .extract()?;
        Ok(config)
    }

    /// The data-bridge idle timeout as a [`std::time::Duration`].
    pub fn data_idle(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.data_idle_secs)
    }
}

fn default_scope() -> String {
    "read:user".to_string()
}
fn default_database_url() -> String {
    "sqlite://pocket-codex.db".to_string()
}
fn default_relay_addr() -> String {
    "127.0.0.1:7666".to_string()
}
fn default_http_listen() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_broker_listen() -> String {
    "0.0.0.0:7900".to_string()
}
fn default_jwt_ttl() -> i64 {
    3600
}
fn default_refresh_ttl() -> i64 {
    30 * 24 * 3600
}
fn default_data_idle() -> u64 {
    1800
}
