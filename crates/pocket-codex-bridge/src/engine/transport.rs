//! Which relay this device talks to, with which credential, under which key
//! namespace.
//!
//! ```text
//!   signed in?  ─yes─▶ account:   GET /v1/relay, keys under `pcxu:<user>:`
//!         │ no
//!         ▼
//!                      self-host: configured relay + stored key, `pcx:` keys
//! ```
//!
//! # Why this exists
//!
//! Account mode used to be a wholly separate implementation: its bytes went
//! through the backend's broker, so every subscribe, probe, and register had an
//! account-shaped variant beside its self-host one. With clients reaching the
//! relay directly the two differ in exactly two values — the credential and
//! whether keys carry an account namespace — so both collapse into this one
//! type and every caller downstream has a single path.
//!
//! It also replaces `apply_key`, which installed the relay key into a
//! PROCESS-GLOBAL before each call. The credential is a value here, so nothing
//! ambient has to be set up first and two sessions can coexist.

use anyhow::{anyhow, Result};
use pocket_codex_account_proto::key::NamespacedServiceId;
use pocket_codex_core::{config::Mode, service::ServiceId};
use pocket_codex_pb::RelaySession;

use crate::engine::{account, config::load_config, runtime};

/// How this device reaches Pocket-Codex services.
#[derive(Debug, Clone)]
pub struct Transport {
    /// The relay to dial and the credential to present.
    pub session: RelaySession,
    /// The account id to namespace keys under, or `None` in self-host mode.
    pub namespace: Option<String>,
}

impl Transport {
    /// The relay key for `service` under this transport.
    ///
    /// The one place the two modes' key shapes are decided, so a device cannot
    /// register under one shape and subscribe under the other.
    pub fn key(&self, service: &ServiceId) -> String {
        match &self.namespace {
            Some(ns) => NamespacedServiceId::new(ns, service.clone()).key(),
            None => service.key(),
        }
    }

    /// Parse a relay key back into a [`ServiceId`], accepting whichever shape
    /// this transport emits. The inverse of [`Self::key`].
    pub fn parse_key(&self, key: &str) -> Option<ServiceId> {
        match self.namespace.is_some() {
            true => NamespacedServiceId::parse_key(key).map(|nsid| nsid.service),
            false => ServiceId::parse_key(key),
        }
    }

    /// Re-key a service key that may be in EITHER shape into this transport's.
    ///
    /// The app's own state and its Dart layer both carry bare `pcx:` keys as a
    /// service's stable identity, while the relay in account mode wants the
    /// namespaced form. This converts without the caller having to know which
    /// it was handed; an unparsable key is passed through so an explicit
    /// key a user typed still reaches the relay verbatim.
    pub fn relay_key(&self, service_key: &str) -> String {
        ServiceId::parse_key(service_key)
            .or_else(|| NamespacedServiceId::parse_key(service_key).map(|n| n.service))
            .map(|service| self.key(&service))
            .unwrap_or_else(|| service_key.to_string())
    }
}

/// Resolve this device's transport from its persisted configuration.
///
/// Async because account mode fetches a credential from the backend; that
/// result is cached (see [`account::relay_credential`]), so repeated calls are
/// cheap.
pub async fn resolve() -> Result<Transport> {
    let support = runtime::support_dir()?;
    let config = load_config(&support)?;
    if config.account_mode() == Mode::Account {
        let (session, expires_at) = account::relay_session(&support).await?;
        // A registration outlives the call that made it, and the relay cancels a
        // lapsed credential's tunnels — so the refresher has to be running for
        // hosting to survive its TTL. Idempotent, hence unconditional here.
        account::start_credential_refresh(&support, expires_at);
        return Ok(Transport {
            session,
            namespace: Some(account::relay_credential(&support).await?.namespace),
        });
    }
    let relay = config
        .relay()
        .ok_or_else(|| anyhow!("no relay configured"))?
        .to_string();
    let key = config
        .relay_key()
        .ok_or_else(|| anyhow!("no key configured"))?;
    // Length-checked here so a hand-edited config.toml cannot reach the SDK's own
    // error, which echoes the raw key into its message.
    if key.len() != 32 {
        return Err(anyhow!("stored MSG_HEADER_KEY is not 32 bytes; re-run setup"));
    }
    Ok(Transport {
        session: RelaySession::new(relay, key),
        namespace: None,
    })
}

/// Resolve the transport from the engine runtime, for the synchronous
/// flutter_rust_bridge entrypoints.
pub fn resolve_blocking() -> Result<Transport> {
    runtime::runtime().block_on(resolve())
}

#[cfg(test)]
mod tests {
    use pocket_codex_core::service::ServiceKind;

    use super::*;

    fn account() -> Transport {
        Transport {
            session: RelaySession::for_test("relay.example:7666"),
            namespace: Some("bob".to_string()),
        }
    }

    fn self_host() -> Transport {
        Transport {
            session: RelaySession::for_test("relay.example:7666"),
            namespace: None,
        }
    }

    #[test]
    fn account_keys_are_namespaced_and_self_host_keys_are_not() {
        let service = ServiceId::new("studio", ServiceKind::App, "default");
        assert_eq!(account().key(&service), "pcxu:bob:studio:app:default");
        assert_eq!(self_host().key(&service), "pcx:studio:app:default");
    }

    #[test]
    fn parse_key_inverts_key_in_both_modes() {
        for transport in [account(), self_host()] {
            let service = ServiceId::new("studio", ServiceKind::Api, "work");
            let parsed = transport
                .parse_key(&transport.key(&service))
                .expect("a key this transport emitted must parse back");
            assert_eq!(parsed, service);
        }
    }

    #[test]
    fn relay_key_accepts_either_shape_and_normalises_to_this_transport() {
        // The app's state carries bare `pcx:` keys; account mode has to reach the
        // relay with the namespaced form. Both inputs must land on the same key.
        let bare = "pcx:studio:app:default";
        let namespaced = "pcxu:bob:studio:app:default";
        assert_eq!(account().relay_key(bare), namespaced);
        assert_eq!(account().relay_key(namespaced), namespaced);
        assert_eq!(self_host().relay_key(namespaced), bare);
        assert_eq!(self_host().relay_key(bare), bare);
    }

    #[test]
    fn relay_key_passes_an_unrecognised_key_through_unchanged() {
        // A key a user typed by hand is theirs to name; rewriting it would send
        // the request somewhere they did not ask for.
        assert_eq!(self_host().relay_key("custom-key"), "custom-key");
        assert_eq!(account().relay_key("custom-key"), "custom-key");
    }
}
