//! Configuration schema for Pocket-Codex.
//!
//! ```text
//!                          Config (config.toml)
//!                                   │
//!         ┌─────────────────────────┼─────────────────────────┐
//!         ▼                         ▼                         ▼
//!       codex                   pb_mapper                 services
//!     CodexConfig              PbMapperConfig            ServicesConfig
//!         │                         │                         │
//!         ▼                    ┌────┴────┐          ┌────────┴────────┐
//!     binary?               relay?     key?         ▼                 ▼
//!     (path to           (host:port  (shared      app               api
//!      `codex` binary)   of relay)  MSG_HEADER  ServicePreference  ServicePreference
//!                                    _KEY)
//!                                                   │                 │
//!                                                   ▼                 ▼
//!                                                default?          default?
//!                                                  │                 │
//!                                                  ▼                 ▼
//!                                          ServiceTargetConfig {device, name}
//! ```
//!
//! The CLI loads this TOML file from
//! `$XDG_CONFIG_HOME/pocket-codex/config.toml` (resolved through
//! [`crate::paths::config_file`]) and merges it with command-line
//! flags. `services.{app,api}.default` is the per-kind preferred
//! target consulted by `choose_target` before falling back to
//! `state.selected_service` or relay discovery.

use serde::{Deserialize, Serialize};

use crate::{
    error::Result,
    paths,
    service::{sanitize_component, ServiceKind},
};

/// Top-level Pocket-Codex configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Settings for the local `codex app-server` process.
    #[serde(default)]
    pub codex: CodexConfig,

    /// Settings for the `pb-mapper` register/subscribe layer.
    #[serde(default)]
    pub pb_mapper: PbMapperConfig,

    /// Local service selection preferences.
    #[serde(default)]
    pub services: ServicesConfig,

    /// UI preferences (front-end only).
    #[serde(default)]
    pub ui: UiConfig,

    /// Hosted-account (GitHub login) settings. Absent in older configs, which
    /// load as the default (self-host / unconfigured).
    #[serde(default)]
    pub account: AccountConfig,
}

/// Which transport the client uses to reach Pocket-Codex services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Hosted account via the GitHub-login backend — the default experience.
    Account,
    /// Self-hosted pb-mapper relay (relay `host:port` + `MSG_HEADER_KEY`) —
    /// the advanced experience.
    SelfHost,
    /// Neither configured yet (first run).
    Unconfigured,
}

/// Hosted-account configuration. The session [`AccountConfig::token`] is a
/// backend-issued bearer credential (never the GitHub token) and is stored in
/// the same `0o600` `config.toml` as the relay key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    /// Explicit mode override. Only `"self_host"` is honoured (the user toggled
    /// to advanced mode); otherwise the mode is derived from whether a token /
    /// relay is present. `None` for normal account use.
    pub mode: Option<String>,

    /// Identity provider. Currently only `"github"`.
    pub provider: Option<String>,

    /// Backend base URL override (e.g. for self-hosting the backend or dev).
    /// `None` falls back to the client's compile-time default.
    pub backend: Option<String>,

    /// Backend session bearer token (a JWT). Sensitive; persisted 0600.
    pub token: Option<String>,

    /// Opaque refresh token used to renew [`Self::token`] when it expires.
    /// Sensitive; persisted 0600.
    pub refresh_token: Option<String>,

    /// Signed-in GitHub login/handle (display only).
    pub login: Option<String>,

    /// GitHub account id (opaque; display/debug only).
    pub account_id: Option<String>,
}

/// Front-end UI preferences. Engine code ignores these; they exist so the
/// Flutter app can persist user choices through the one config channel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiConfig {
    /// BCP-47 language code the app should use (e.g. `en`, `zh`). `None`
    /// means follow the system locale.
    pub locale: Option<String>,
}

/// Configuration for managing the local `codex app-server` process.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexConfig {
    /// Optional explicit path to the `codex` binary. If unset, the
    /// process manager will look it up on `PATH`.
    pub binary: Option<String>,
}

/// Configuration for the `pb-mapper` integration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PbMapperConfig {
    /// Bare `host:port` of the upstream `pb-mapper` relay
    /// (e.g. `relay.example.com:7666`).
    pub relay: Option<String>,

    /// Shared 32-byte `MSG_HEADER_KEY` the relay validates every control
    /// message against. Stored here so commands default to it without an
    /// exported environment variable. Length validation (32 bytes) lives in
    /// the CLI layer; this field stores the value verbatim.
    pub key: Option<String>,
}

/// User-configured service preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicesConfig {
    /// Preferences for app-server remote control.
    #[serde(default)]
    pub app: ServicePreference,

    /// Preferences for direct Responses API proxying.
    #[serde(default)]
    pub api: ServicePreference,
}

/// Preferences for one service kind.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePreference {
    /// Default target to use when the CLI invocation does not specify one.
    pub default: Option<ServiceTargetConfig>,
}

/// Device/name pair persisted in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceTargetConfig {
    /// Default device id.
    pub device: String,

    /// Default service instance name.
    pub name: String,
}

impl Config {
    /// Load configuration from the default location.
    ///
    /// Missing config files are treated as empty/default configuration.
    pub fn load() -> Result<Self> {
        let path = paths::config_file()?;
        match std::fs::read_to_string(path) {
            Ok(raw) => Ok(toml::from_str(&raw)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    /// Persist configuration to the default location. On unix the file is
    /// created with `0o600` and any pre-existing file is tightened to
    /// `0o600` *before* the bytes are written, because it may hold the
    /// relay `MSG_HEADER_KEY` or the account session/refresh tokens.
    pub fn save(&self) -> Result<()> {
        let path = paths::config_file()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;

        #[cfg(unix)]
        {
            use std::{
                io::Write as _,
                os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
            };
            // `mode(0o600)` covers the create case. For a pre-existing file
            // `open` keeps its old (possibly world-readable) mode, so tighten
            // the open handle to 0o600 *before* writing — the secret is never
            // on disk while the file is group/world-readable.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            file.write_all(raw.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, raw)?;
        }
        Ok(())
    }

    /// Return the configured default target for a service kind.
    pub fn default_service(&self, kind: ServiceKind) -> Option<&ServiceTargetConfig> {
        match kind {
            ServiceKind::App => self.services.app.default.as_ref(),
            ServiceKind::Api => self.services.api.default.as_ref(),
            // The meta service is always colocated with an app/api host and its
            // key is derived from theirs, so it has no standalone default target.
            ServiceKind::Meta => None,
        }
    }

    /// Set the configured default target for a service kind.
    pub fn set_default_service(
        &mut self,
        kind: ServiceKind,
        device: impl AsRef<str>,
        name: impl AsRef<str>,
    ) {
        let target = ServiceTargetConfig {
            device: sanitize_component(device.as_ref()),
            name: sanitize_component(name.as_ref()),
        };
        match kind {
            ServiceKind::App => self.services.app.default = Some(target),
            ServiceKind::Api => self.services.api.default = Some(target),
            // No standalone default for the colocated/derived meta service.
            ServiceKind::Meta => {},
        }
    }

    /// Configured relay `host:port`, or `None` when unset/blank.
    pub fn relay(&self) -> Option<&str> {
        self.pb_mapper
            .relay
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Configured `MSG_HEADER_KEY`, or `None` when unset/blank.
    pub fn relay_key(&self) -> Option<&str> {
        self.pb_mapper
            .key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Set the relay `host:port`. A blank value clears it.
    pub fn set_relay(&mut self, relay: impl AsRef<str>) {
        let trimmed = relay.as_ref().trim();
        self.pb_mapper.relay = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    /// Set the shared `MSG_HEADER_KEY`. A blank value clears it.
    pub fn set_relay_key(&mut self, key: impl AsRef<str>) {
        let trimmed = key.as_ref().trim();
        self.pb_mapper.key = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    /// Configured UI locale (BCP-47, e.g. `en`/`zh`), or `None` to follow
    /// the system locale.
    pub fn locale(&self) -> Option<&str> {
        self.ui
            .locale
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Set the UI locale. A blank value clears it (= follow system).
    pub fn set_locale(&mut self, locale: impl AsRef<str>) {
        let trimmed = locale.as_ref().trim();
        self.ui.locale = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    /// The active transport mode.
    ///
    /// An explicit `account.mode = "self_host"` always wins (the user toggled
    /// to advanced mode). Otherwise: a stored account token means
    /// [`Mode::Account`], a configured relay means [`Mode::SelfHost`], and
    /// neither is [`Mode::Unconfigured`].
    pub fn account_mode(&self) -> Mode {
        if self.account.mode.as_deref().map(str::trim) == Some("self_host") {
            return Mode::SelfHost;
        }
        if self.account_token().is_some() {
            Mode::Account
        } else if self.relay().is_some() {
            Mode::SelfHost
        } else {
            Mode::Unconfigured
        }
    }

    /// Force a transport mode. [`Mode::SelfHost`] records the override (so it
    /// sticks even with a token present);
    /// [`Mode::Account`]/[`Mode::Unconfigured`] clear the override and let
    /// [`Self::account_mode`] derive it.
    pub fn set_mode(&mut self, mode: Mode) {
        self.account.mode = match mode {
            Mode::SelfHost => Some("self_host".to_string()),
            Mode::Account | Mode::Unconfigured => None,
        };
    }

    /// Backend session token, or `None` when unset/blank.
    pub fn account_token(&self) -> Option<&str> {
        self.account
            .token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Refresh token used to renew the session token, or `None` when unset.
    pub fn account_refresh_token(&self) -> Option<&str> {
        self.account
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Signed-in GitHub login, or `None`.
    pub fn account_login(&self) -> Option<&str> {
        self.account
            .login
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Configured backend base URL override, or `None` (use the default).
    pub fn account_backend(&self) -> Option<&str> {
        self.account
            .backend
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Set the backend base URL override. A blank value clears it.
    pub fn set_account_backend(&mut self, backend: impl AsRef<str>) {
        let trimmed = backend.as_ref().trim();
        self.account.backend = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    /// Record a freshly issued session (after login or refresh) and switch to
    /// account mode by clearing any self-host override.
    pub fn set_account_session(
        &mut self,
        token: impl AsRef<str>,
        refresh_token: impl AsRef<str>,
        login: impl AsRef<str>,
        account_id: Option<String>,
    ) {
        self.account.token = Some(token.as_ref().to_string());
        self.account.refresh_token = Some(refresh_token.as_ref().to_string());
        self.account.login = Some(login.as_ref().to_string());
        self.account.account_id = account_id.filter(|s| !s.trim().is_empty());
        self.account.provider = Some("github".to_string());
        self.account.mode = None;
    }

    /// Clear the account session (logout): drops the tokens, identity and any
    /// mode override, leaving the self-host relay config untouched.
    pub fn clear_account(&mut self) {
        self.account.token = None;
        self.account.refresh_token = None;
        self.account.login = None;
        self.account.account_id = None;
        self.account.provider = None;
        self.account.mode = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::ServiceKind;

    #[test]
    fn service_default_roundtrips_in_config() {
        let mut config = Config::default();
        config.set_default_service(ServiceKind::App, "studio", "work");

        let target = config
            .default_service(ServiceKind::App)
            .expect("app default target");

        assert_eq!(target.device, "studio");
        assert_eq!(target.name, "work");
        assert!(config.default_service(ServiceKind::Api).is_none());
    }

    #[test]
    fn relay_and_key_roundtrip_in_config() {
        let mut config = Config::default();
        assert_eq!(config.relay(), None);
        assert_eq!(config.relay_key(), None);

        config.set_relay("lb7666.top:7666");
        config.set_relay_key("0123456789abcdef0123456789abcdef");

        assert_eq!(config.relay(), Some("lb7666.top:7666"));
        assert_eq!(config.relay_key(), Some("0123456789abcdef0123456789abcdef"));

        let raw = toml::to_string_pretty(&config).expect("serialize");
        let reloaded: Config = toml::from_str(&raw).expect("deserialize");
        assert_eq!(reloaded.relay(), Some("lb7666.top:7666"));
        assert_eq!(reloaded.relay_key(), Some("0123456789abcdef0123456789abcdef"));
    }

    #[test]
    fn empty_relay_or_key_is_treated_as_unset() {
        let mut config = Config::default();
        config.set_relay("   ");
        config.set_relay_key("");
        assert_eq!(config.relay(), None);
        assert_eq!(config.relay_key(), None);
    }

    #[test]
    fn accessors_trim_values_loaded_from_toml() {
        // A hand-edited config with surrounding whitespace must still resolve
        // to the trimmed value through the read-side accessors.
        let raw = "[pb_mapper]\nrelay = \"  lb7666.top:7666  \"\nkey = \"  abc  \"\n";
        let config: Config = toml::from_str(raw).expect("deserialize");
        assert_eq!(config.relay(), Some("lb7666.top:7666"));
        assert_eq!(config.relay_key(), Some("abc"));
    }

    #[test]
    fn locale_roundtrips_and_old_config_defaults_to_none() {
        // A config with no [ui] section (older builds) loads with locale None.
        let old: Config = toml::from_str("[pb_mapper]\nrelay = \"r:1\"\n").expect("deserialize");
        assert_eq!(old.locale(), None);

        let mut config = Config::default();
        config.set_locale("en");
        assert_eq!(config.locale(), Some("en"));
        let raw = toml::to_string_pretty(&config).expect("serialize");
        let reloaded: Config = toml::from_str(&raw).expect("deserialize");
        assert_eq!(reloaded.locale(), Some("en"));

        // Blank clears it (= follow system).
        config.set_locale("  ");
        assert_eq!(config.locale(), None);
    }

    #[test]
    fn account_mode_derives_from_token_and_relay() {
        // Nothing configured → unconfigured.
        let mut config = Config::default();
        assert_eq!(config.account_mode(), Mode::Unconfigured);

        // Relay only → self-host.
        config.set_relay("lb7666.top:7666");
        assert_eq!(config.account_mode(), Mode::SelfHost);

        // A token present → account, even with a relay configured.
        config.set_account_session("jwt", "refresh", "octocat", Some("42".into()));
        assert_eq!(config.account_mode(), Mode::Account);
        assert_eq!(config.account_token(), Some("jwt"));
        assert_eq!(config.account_refresh_token(), Some("refresh"));
        assert_eq!(config.account_login(), Some("octocat"));

        // Explicit self-host override wins even with a token present.
        config.set_mode(Mode::SelfHost);
        assert_eq!(config.account_mode(), Mode::SelfHost);

        // Switching back to account clears the override (token still present).
        config.set_mode(Mode::Account);
        assert_eq!(config.account_mode(), Mode::Account);
    }

    #[test]
    fn account_session_roundtrips_and_logout_reverts() {
        let mut config = Config::default();
        config.set_relay("r:1");
        config.set_account_session("tok", "ref", "octocat", None);

        let raw = toml::to_string_pretty(&config).expect("serialize");
        let reloaded: Config = toml::from_str(&raw).expect("deserialize");
        assert_eq!(reloaded.account_mode(), Mode::Account);
        assert_eq!(reloaded.account_login(), Some("octocat"));

        // Logout drops the account but keeps the self-host relay.
        let mut config = reloaded;
        config.clear_account();
        assert_eq!(config.account_token(), None);
        assert_eq!(config.relay(), Some("r:1"));
        assert_eq!(config.account_mode(), Mode::SelfHost);
    }

    #[test]
    fn old_config_without_account_section_loads() {
        // Older builds wrote no [account] table; it must still parse.
        let old: Config = toml::from_str("[pb_mapper]\nrelay = \"r:1\"\n").expect("deserialize");
        assert_eq!(old.account_mode(), Mode::SelfHost);
        assert_eq!(old.account_token(), None);
    }

    #[test]
    fn account_backend_override_trims_and_clears() {
        let mut config = Config::default();
        assert_eq!(config.account_backend(), None);
        config.set_account_backend("  https://api.lb7666.top  ");
        assert_eq!(config.account_backend(), Some("https://api.lb7666.top"));
        config.set_account_backend("");
        assert_eq!(config.account_backend(), None);
    }
}
