//! Transport resolution: which relay to talk to, with which credential, and
//! under which key namespace.
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
//! # Why one type, not two
//!
//! Account mode used to be a different transport in kind: its bytes went
//! through the backend's broker, so it needed a TLS connector, a token
//! provider, a tunnel protocol, and its own register/subscribe implementations.
//! Now that clients reach the relay directly, the two modes differ in exactly
//! two values — which credential to present, and whether service keys carry an
//! account namespace. So this is one struct, and every command downstream has a
//! single code path instead of a `match` with two parallel implementations.

use anyhow::{anyhow, Result};
use pocket_codex_account_proto::key::NamespacedServiceId;
use pocket_codex_core::{
    config::{Config, Mode},
    service::ServiceId,
};

use crate::commands::{account, relay};

/// How a command reaches Pocket-Codex services.
///
/// `Debug` comes from [`pocket_codex_pb::RelaySession`]'s, which redacts the
/// credential — so a transport can appear in a log line or a test assertion
/// without leaking the key it carries.
#[derive(Debug)]
pub(crate) struct Transport {
    /// The relay address and the credential to present to it.
    ///
    /// In account mode this credential came from `/v1/relay` and the relay
    /// confines it to the account's namespace; in self-host mode it is the
    /// operator's own key for their own relay.
    pub session: pocket_codex_pb::RelaySession,
    /// The account id to namespace service keys under, or `None` in self-host
    /// mode where keys are un-namespaced `pcx:` ones.
    pub namespace: Option<String>,
}

impl Transport {
    /// The relay key for `service` under this transport.
    ///
    /// The one place the two modes' key shapes are decided, so a caller cannot
    /// register under one shape and subscribe under the other.
    pub(crate) fn key(&self, service: &ServiceId) -> String {
        match &self.namespace {
            Some(ns) => NamespacedServiceId::new(ns, service.clone()).key(),
            None => service.key(),
        }
    }

    /// Parse a relay key back into a [`ServiceId`], accepting whichever shape
    /// this transport emits. The inverse of [`Self::key`].
    pub(crate) fn parse_key(&self, key: &str) -> Option<ServiceId> {
        match self.namespace.is_some() {
            true => NamespacedServiceId::parse_key(key).map(|nsid| nsid.service),
            false => ServiceId::parse_key(key),
        }
    }

    /// Whether this is a hosted account (as opposed to a self-hosted relay).
    pub(crate) fn is_account(&self) -> bool {
        self.namespace.is_some()
    }
}

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
    use pocket_codex_core::service::ServiceKind;

    use super::*;

    fn account_transport() -> Transport {
        Transport {
            session: pocket_codex_pb::RelaySession::for_test("relay.example:7666"),
            namespace: Some("bob".to_string()),
        }
    }

    fn self_host_transport() -> Transport {
        Transport {
            session: pocket_codex_pb::RelaySession::for_test("relay.example:7666"),
            namespace: None,
        }
    }

    #[test]
    fn account_keys_are_namespaced_and_self_host_keys_are_not() {
        let service = ServiceId::new("studio", ServiceKind::App, "default");
        assert_eq!(account_transport().key(&service), "pcxu:bob:studio:app:default");
        assert_eq!(self_host_transport().key(&service), "pcx:studio:app:default");
    }

    #[test]
    fn parse_key_inverts_key_in_both_modes() {
        // Register writes a key and connect reads one back; if these ever
        // disagreed a client would subscribe to a name nothing published.
        for transport in [account_transport(), self_host_transport()] {
            let service = ServiceId::new("studio", ServiceKind::Api, "work");
            let parsed = transport
                .parse_key(&transport.key(&service))
                .expect("a key this transport emitted must parse back");
            assert_eq!(parsed, service);
        }
    }

    #[test]
    fn each_mode_rejects_the_other_shape() {
        // Not pedantry: an account client that accepted a bare `pcx:` key would
        // list a self-hoster's services as its own in a shared-relay listing.
        assert!(account_transport()
            .parse_key("pcx:studio:app:default")
            .is_none());
        assert!(self_host_transport()
            .parse_key("pcxu:bob:studio:app:default")
            .is_none());
    }

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
