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
//!         ▼                         ▼                ┌────────┴────────┐
//!     binary?                   relay?               ▼                 ▼
//!     (path to                  (relay URL,        app               api
//!      `codex` binary)           e.g. tcp://…)  ServicePreference  ServicePreference
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
    /// URL of the upstream `pb-mapper` relay (e.g.
    /// `tcp://relay.example.com:7800`).
    pub relay: Option<String>,
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

    /// Persist configuration to the default location.
    pub fn save(&self) -> Result<()> {
        let path = paths::config_file()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(path, raw)?;
        Ok(())
    }

    /// Return the configured default target for a service kind.
    pub fn default_service(&self, kind: ServiceKind) -> Option<&ServiceTargetConfig> {
        match kind {
            ServiceKind::App => self.services.app.default.as_ref(),
            ServiceKind::Api => self.services.api.default.as_ref(),
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
        }
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
}
