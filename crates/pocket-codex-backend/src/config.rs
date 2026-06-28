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
    /// Omit (or leave blank) to use the relay's machine-derived key — required
    /// when the relay runs with `--use-machine-msg-header-key` and the backend
    /// is on the same host.
    #[serde(default)]
    pub msg_header_key: Option<String>,
    /// HS256 secret used to sign session JWTs.
    pub jwt_secret: String,
    /// GitHub OAuth app client id (Device Flow enabled).
    pub github_client_id: String,
    /// GitHub OAuth app client secret. Required ONLY for the web
    /// (authorization-code / browser-redirect) login flow; the device flow
    /// needs none. Leave unset to keep the web flow disabled (its endpoints
    /// then return 503 while the device flow keeps working).
    #[serde(default)]
    pub github_client_secret: Option<String>,
    /// Public base URL the browser reaches this backend at (e.g.
    /// `https://lb7666.top:8443`). The web flow's OAuth callback is
    /// `{public_url}/auth/web/callback`, which must EXACTLY match the GitHub
    /// OAuth app's registered Authorization callback URL. Required (with
    /// `github_client_secret`) to enable the web flow.
    #[serde(default)]
    pub public_url: Option<String>,
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
    /// Load config from the TOML file at `$POCKET_CODEX_BACKEND_CONFIG`
    /// (default `backend.toml`, optional) layered under `PCX_`-prefixed env
    /// vars.
    pub fn load() -> anyhow::Result<Self> {
        use figment::{
            providers::{Env, Format, Toml},
            Figment,
        };
        let path = std::env::var("POCKET_CODEX_BACKEND_CONFIG")
            .unwrap_or_else(|_| "backend.toml".to_string());
        let config: Self = Figment::new()
            .merge(Toml::file(path))
            .merge(Env::prefixed("PCX_"))
            .extract()?;
        config.validate()?;
        Ok(config)
    }

    /// Fail closed on a missing, weak, or still-placeholder secret. A forgeable
    /// `jwt_secret` collapses the entire per-account isolation (the broker
    /// derives the relay-key namespace from the JWT's `sub`), so the backend
    /// must refuse to boot rather than sign tokens anyone can forge — including
    /// the case where an operator deploys the example env without editing it.
    fn validate(&self) -> anyhow::Result<()> {
        use anyhow::ensure;
        ensure!(
            !is_placeholder(&self.jwt_secret),
            "PCX_JWT_SECRET is unset or still the example placeholder; set a real secret (e.g. \
             `openssl rand -hex 32`)"
        );
        ensure!(
            self.jwt_secret.len() >= 32,
            "PCX_JWT_SECRET must be at least 32 bytes of random material (got {} bytes)",
            self.jwt_secret.len()
        );
        ensure!(
            !is_placeholder(&self.github_client_id),
            "PCX_GITHUB_CLIENT_ID is unset or still the example placeholder"
        );
        if let Some(key) = &self.msg_header_key {
            ensure!(
                !key.contains("replace-with"),
                "PCX_MSG_HEADER_KEY is still the example placeholder; set the relay's 32-byte key \
                 or leave it unset to adopt the relay's machine-derived key"
            );
        }
        // Web flow is opt-in: a shipped placeholder secret must fail closed
        // rather than silently enable a half-configured flow. Both the secret
        // and the public callback URL are needed together.
        if let Some(secret) = &self.github_client_secret {
            ensure!(
                !is_placeholder(secret),
                "PCX_GITHUB_CLIENT_SECRET is still the example placeholder; set a real secret to \
                 enable web login, or remove it to keep web login disabled"
            );
            ensure!(
                self.public_url.as_deref().is_some_and(|u| !is_placeholder(u)),
                "PCX_GITHUB_CLIENT_SECRET is set but PCX_PUBLIC_URL is missing; the web flow needs \
                 the public callback base URL (e.g. https://lb7666.top:8443)"
            );
        }
        Ok(())
    }

    /// The data-bridge idle timeout as a [`std::time::Duration`].
    pub fn data_idle(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.data_idle_secs)
    }
}

/// True for an empty/whitespace value or one that still carries the shipped
/// `replace-with-…` example sentinel.
fn is_placeholder(value: &str) -> bool {
    value.trim().is_empty() || value.contains("replace-with")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> ServerConfig {
        ServerConfig {
            msg_header_key: None,
            jwt_secret: "x".repeat(32),
            github_client_id: "Iv1.0123456789abcdef".to_string(),
            github_client_secret: None,
            public_url: None,
            github_scope: default_scope(),
            database_url: default_database_url(),
            relay_addr: default_relay_addr(),
            http_listen: default_http_listen(),
            broker_listen: default_broker_listen(),
            jwt_ttl_secs: default_jwt_ttl(),
            refresh_ttl_secs: default_refresh_ttl(),
            data_idle_secs: default_data_idle(),
            tls_mode: TlsMode::Plain,
            tls_cert: None,
            tls_key: None,
        }
    }

    #[test]
    fn accepts_a_real_config() {
        valid()
            .validate()
            .expect("a fully-specified config should validate");
    }

    #[test]
    fn rejects_the_placeholder_jwt_secret() {
        let mut c = valid();
        c.jwt_secret = "replace-with-a-long-random-secret".to_string();
        assert!(c.validate().is_err(), "shipped placeholder must fail closed");
    }

    #[test]
    fn rejects_a_short_jwt_secret() {
        let mut c = valid();
        c.jwt_secret = "deadbeef".to_string(); // 8 bytes
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_empty_jwt_secret_and_client_id() {
        let mut c = valid();
        c.jwt_secret = String::new();
        assert!(c.validate().is_err());
        let mut c = valid();
        c.github_client_id = "   ".to_string();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_the_placeholder_msg_header_key_but_allows_absent() {
        let mut c = valid();
        c.msg_header_key = Some("replace-with-the-32-byte-relay-key".to_string());
        assert!(c.validate().is_err());
        c.msg_header_key = None;
        c.validate()
            .expect("an absent key adopts the relay machine key");
    }

    #[test]
    fn web_flow_secret_requires_public_url_and_rejects_placeholder() {
        // Secret set but no public_url → fail closed.
        let mut c = valid();
        c.github_client_secret = Some("a-real-looking-secret".to_string());
        assert!(c.validate().is_err());
        // Both set → valid (web flow enabled).
        c.public_url = Some("https://lb7666.top:8443".to_string());
        c.validate().expect("secret + public_url enables web flow");
        // A placeholder secret fails closed even with a public_url.
        c.github_client_secret = Some("replace-with-github-oauth-client-secret".to_string());
        assert!(c.validate().is_err());
    }
}
