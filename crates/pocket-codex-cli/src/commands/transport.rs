//! How a CLI command resolves its [`Transport`] from flags and saved config.
//!
//! ```text
//!   --relay given?  ─yes─▶ self-host: saved/env credential, un-namespaced keys
//!         │ no
//!         ▼
//!   signed in?      ─yes─▶ account:  GET /v1/relay, keys under `pcxu:<user>:`
//!         │ no
//!         ▼
//!                          self-host
//! ```
//!
//! The type itself, and the key-shape rules that must hold identically here and
//! in the app, live in [`pocket_codex_pb::transport`]. This module is only the
//! CLI's half: reading flags and a `Config`. The app resolves the same type
//! from its support directory instead.

use anyhow::{anyhow, Result};
use pocket_codex_core::config::{Config, Mode};
// Re-exported so callers keep saying `commands::transport::Transport`: for a
// command, the resolver and the type it returns are one concept.
pub(crate) use pocket_codex_pb::Transport;

use crate::commands::{account, relay};

/// Resolve the transport for a command.
///
/// An explicit `--relay` always forces self-host — the escape hatch, even when
/// signed in — because a caller naming a relay means that relay, not their
/// account's.
pub(crate) async fn resolve_transport(
    relay_flag: Option<&str>,
    backend_flag: Option<&str>,
    config: &Config,
) -> Result<Transport> {
    if relay_flag.map(str::trim).is_some_and(|s| !s.is_empty()) {
        return self_host(relay_flag, config);
    }
    match config.account_mode() {
        Mode::Account => {
            if config.account_token().is_none() {
                return Err(anyhow!("account mode but not signed in; run `pocket-codex login`"));
            }
            let backend = account::backend_base(backend_flag, config);
            let mut config = config.clone();
            let relay = account::fetch_relay_credential(&mut config, &backend).await?;
            Ok(Transport {
                session: pocket_codex_pb::RelaySession::new(relay.relay_addr, relay.credential),
                namespace: Some(relay.namespace),
            })
        },
        Mode::SelfHost | Mode::Unconfigured => self_host(relay_flag, config),
    }
}

fn self_host(relay_flag: Option<&str>, config: &Config) -> Result<Transport> {
    Ok(Transport {
        session: relay::resolve_session(relay_flag, config)?,
        namespace: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Key-shape tests live with the type, in `pocket_codex_pb::transport`. These
    // cover only what this module adds: how flags and saved config pick a mode.

    #[tokio::test]
    async fn explicit_relay_forces_self_host_even_when_signed_in() {
        let mut config = Config::default();
        config.set_account_session("tok", "ref", "octocat", None);
        std::env::set_var(relay::CREDENTIAL_ENV, "0".repeat(32));
        let transport = resolve_transport(Some("relay.example:7666"), None, &config)
            .await
            .expect("resolve");
        std::env::remove_var(relay::CREDENTIAL_ENV);
        assert_eq!(transport.session.relay_addr, "relay.example:7666");
        assert!(!transport.is_account(), "an explicit --relay is self-host");
    }

    #[tokio::test]
    async fn self_host_when_only_relay_configured() {
        let mut config = Config::default();
        config.set_relay("relay.example:7666");
        config.set_relay_key("0".repeat(32));
        let transport = resolve_transport(None, None, &config)
            .await
            .expect("resolve");
        assert_eq!(transport.session.relay_addr, "relay.example:7666");
        assert!(!transport.is_account());
    }

    #[tokio::test]
    async fn an_unconfigured_install_names_the_fix_instead_of_dialling() {
        // Neither signed in nor pointed at a relay. Nothing here should touch the
        // network: the user has to make a choice first, and the error has to say
        // which.
        let err = resolve_transport(None, None, &Config::default())
            .await
            .expect_err("an unconfigured install cannot resolve a transport");
        let err = err.to_string();
        assert!(
            err.contains("pocket-codex init") || err.contains("--relay"),
            "the error must name the fix: {err}"
        );
    }
}
