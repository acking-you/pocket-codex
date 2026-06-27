//! Transport resolution: account (backend broker) vs self-host (pb-mapper
//! relay).
//!
//! Precedence rule: an explicit `--relay` always forces self-host (the escape
//! hatch, even when signed in); otherwise account mode when a session token is
//! present, else self-host (today's behaviour).

use anyhow::{anyhow, Result};
use pocket_codex_core::config::{Config, Mode};

use crate::commands::{account, relay};

/// The transport a command should use to reach Pocket-Codex services.
pub(crate) enum Transport {
    /// Self-hosted pb-mapper relay (`host:port`).
    SelfHost {
        /// Resolved relay `host:port`.
        relay: String,
    },
    /// Hosted account: the backend base URL. The session token is loaded (and
    /// refreshed) from config by the token provider, not carried here.
    Account {
        /// Backend base URL.
        backend: String,
    },
}

/// Resolve the transport for a command.
pub(crate) fn resolve_transport(
    relay_flag: Option<&str>,
    backend_flag: Option<&str>,
    config: &Config,
) -> Result<Transport> {
    if relay_flag.map(str::trim).is_some_and(|s| !s.is_empty()) {
        return Ok(Transport::SelfHost {
            relay: relay::resolve_relay(relay_flag, config)?,
        });
    }
    match config.account_mode() {
        Mode::Account => {
            // Validate we're actually signed in; the token itself is loaded and
            // refreshed lazily by the token provider.
            if config.account_token().is_none() {
                return Err(anyhow!("account mode but not signed in; run `pocket-codex login`"));
            }
            Ok(Transport::Account {
                backend: account::backend_base(backend_flag, config),
            })
        },
        Mode::SelfHost | Mode::Unconfigured => Ok(Transport::SelfHost {
            relay: relay::resolve_relay(relay_flag, config)?,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_relay_forces_self_host_even_when_signed_in() {
        let mut config = Config::default();
        config.set_account_session("tok", "ref", "octocat", None);
        match resolve_transport(Some("relay.example:7666"), None, &config).expect("resolve") {
            Transport::SelfHost {
                relay,
            } => assert_eq!(relay, "relay.example:7666"),
            Transport::Account {
                ..
            } => panic!("expected self-host"),
        }
    }

    #[test]
    fn account_mode_used_when_signed_in() {
        let mut config = Config::default();
        config.set_account_session("tok", "ref", "octocat", None);
        match resolve_transport(None, None, &config).expect("resolve") {
            Transport::Account {
                backend,
            } => assert!(!backend.is_empty()),
            Transport::SelfHost {
                ..
            } => panic!("expected account"),
        }
    }

    #[test]
    fn self_host_when_only_relay_configured() {
        let mut config = Config::default();
        config.set_relay("relay.example:7666");
        match resolve_transport(None, None, &config).expect("resolve") {
            Transport::SelfHost {
                relay,
            } => assert_eq!(relay, "relay.example:7666"),
            Transport::Account {
                ..
            } => panic!("expected self-host"),
        }
    }
}
