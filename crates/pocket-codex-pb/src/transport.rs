//! Which relay to talk to, with which credential, and under which key
//! namespace.
//!
//! ```text
//!   signed in?  ─yes─▶ account:   credential from the backend's `/v1/relay`,
//!         │                       keys under `pcxu:<user>:`
//!         │ no
//!         ▼
//!                      self-host: the operator's own relay and key, `pcx:` keys
//! ```
//!
//! # Why this type exists
//!
//! Account mode used to be a wholly separate implementation: its bytes went
//! through the backend's broker, so every subscribe, probe, and register had an
//! account-shaped variant beside its self-host one. With clients reaching the
//! relay directly the two differ in exactly two values — the credential and
//! whether keys carry an account namespace — so both collapse into this one
//! type and every caller downstream has a single path.
//!
//! It also replaced `apply_key`, which installed the relay key into a
//! PROCESS-GLOBAL before each call. The credential is a value here, so nothing
//! ambient has to be set up first and two sessions can coexist.
//!
//! # Resolution lives with the caller
//!
//! The CLI reads a `Config` it was handed; the app reads its support directory
//! and may fetch a credential over HTTP. Those have nothing in common but their
//! result, so each builds its own `Transport` and only the key logic — the part
//! that MUST agree between them — is here.

use pocket_codex_account_proto::key::NamespacedServiceId;
use pocket_codex_core::service::ServiceId;

use crate::RelaySession;

/// How a device reaches Pocket-Codex services.
///
/// `Debug` comes from [`RelaySession`]'s, which redacts the credential — so a
/// transport can appear in a log line or a test assertion without leaking the
/// key it carries.
#[derive(Debug, Clone)]
pub struct Transport {
    /// The relay to dial and the credential to present.
    ///
    /// In account mode this came from `/v1/relay` and the relay confines it to
    /// the account's namespace; in self-host mode it is the operator's own key
    /// for their own relay.
    pub session: RelaySession,
    /// The account id to namespace service keys under, or `None` in self-host
    /// mode where keys are un-namespaced `pcx:` ones.
    pub namespace: Option<String>,
}

impl Transport {
    /// The relay key for `service` under this transport.
    ///
    /// The one place the two modes' key shapes are decided. [`Self::key`] and
    /// [`Self::parse_key`] are an inverse pair and must change together: a
    /// device that registered under one shape and subscribed under the other
    /// would silently listen for a name nothing ever published.
    pub fn key(&self, service: &ServiceId) -> String {
        match &self.namespace {
            Some(ns) => NamespacedServiceId::new(ns, service.clone()).key(),
            None => service.key(),
        }
    }

    /// Parse a relay key back into a [`ServiceId`], accepting whichever shape
    /// this transport emits. The inverse of [`Self::key`].
    ///
    /// Deliberately rejects the *other* mode's shape rather than accepting
    /// both: an account client that read a bare `pcx:` key would list a
    /// self-hoster's services as its own on a shared relay.
    pub fn parse_key(&self, key: &str) -> Option<ServiceId> {
        match self.namespace.is_some() {
            true => NamespacedServiceId::parse_key(key).map(|nsid| nsid.service),
            false => ServiceId::parse_key(key),
        }
    }

    /// Whether this is a hosted account (as opposed to a self-hosted relay).
    pub fn is_account(&self) -> bool {
        self.namespace.is_some()
    }

    /// Re-key a service key that may be in EITHER shape into this transport's.
    ///
    /// The app's own state and its Dart layer both carry bare `pcx:` keys as a
    /// service's stable identity, while the relay in account mode wants the
    /// namespaced form. This converts without the caller having to know which
    /// it was handed; an unparsable key is passed through, so a key a user
    /// typed by hand still reaches the relay verbatim rather than being
    /// rewritten into something they did not ask for.
    pub fn relay_key(&self, service_key: &str) -> String {
        ServiceId::parse_key(service_key)
            .or_else(|| NamespacedServiceId::parse_key(service_key).map(|n| n.service))
            .map(|service| self.key(&service))
            .unwrap_or_else(|| service_key.to_string())
    }
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
        // Register writes a key and connect reads one back; if these ever
        // disagreed a client would subscribe to a name nothing published.
        for transport in [account(), self_host()] {
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
        assert!(account().parse_key("pcx:studio:app:default").is_none());
        assert!(self_host()
            .parse_key("pcxu:bob:studio:app:default")
            .is_none());
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
