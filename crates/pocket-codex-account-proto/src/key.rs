//! Per-user namespacing of pb-mapper relay keys.
//!
//! The relay does not understand identity — any holder of the global key can
//! register or subscribe any key string. The hosted backend closes that gap by
//! prefixing every relay key with the authenticated user's id, so account A can
//! never reach account B's services:
//!
//! ```text
//!   pcxu : <user_id> : <device> : <kind> : <name>
//!    │       │           └────────────────────────── a pocket_codex_core::ServiceId
//!    │       └──────────────────────────────────────  authenticated user id (sanitised)
//!    └──────────────────────────────────────────────  SERVICE_NS_PREFIX
//! ```
//!
//! Clients never choose the `pcxu:<user_id>` prefix — the broker derives it
//! from the verified session token and prepends it server-side. `/v1/services`
//! lists relay keys filtered to the caller's
//! [`NamespacedServiceId::user_prefix`].

use pocket_codex_core::service::{sanitize_component, ServiceId};
use serde::{Deserialize, Serialize};

/// Prefix for per-user (account-mode) pb-mapper relay keys. Distinct from the
/// self-host `pcx:` prefix so the two coexist on one relay.
pub const SERVICE_NS_PREFIX: &str = "pcxu";

/// A relay key scoped to one account: a [`ServiceId`] owned by `user_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespacedServiceId {
    /// Authenticated, sanitised account user id.
    pub user_id: String,
    /// The device/kind/name identity of the service within the account.
    pub service: ServiceId,
}

impl NamespacedServiceId {
    /// Build a namespaced id, sanitising `user_id` AND the service segments into
    /// stable key segments. Re-sanitising `service` here (not just trusting the
    /// caller to have built it via [`ServiceId::new`]) makes the namespace
    /// guarantee a property of this security-critical type rather than of caller
    /// discipline: `key()` can never emit a stray `:` that injects extra
    /// segments or another user's prefix.
    pub fn new(user_id: impl AsRef<str>, service: ServiceId) -> Self {
        Self {
            user_id: sanitize_component(user_id.as_ref()),
            service: ServiceId::new(&service.device, service.kind, &service.name),
        }
    }

    /// The full relay key, `pcxu:<user_id>:<device>:<kind>:<name>`.
    pub fn key(&self) -> String {
        format!(
            "{SERVICE_NS_PREFIX}:{}:{}:{}:{}",
            self.user_id,
            self.service.device,
            self.service.kind.as_key_segment(),
            self.service.name
        )
    }

    /// The relay-key prefix shared by all of `user_id`'s services. Used to
    /// filter a relay key listing down to one account.
    pub fn user_prefix(user_id: impl AsRef<str>) -> String {
        format!("{SERVICE_NS_PREFIX}:{}:", sanitize_component(user_id.as_ref()))
    }

    /// Parse a namespaced relay key. Returns `None` for any key that is not a
    /// well-formed `pcxu:<user>:<device>:<kind>:<name>` (so self-host `pcx:`
    /// and generic relay keys are ignored).
    pub fn parse_key(key: &str) -> Option<Self> {
        let mut parts = key.split(':');
        if parts.next()? != SERVICE_NS_PREFIX {
            return None;
        }
        let user_id = parts.next()?;
        let device = parts.next()?;
        let kind = parts.next()?.parse().ok()?;
        let name = parts.next()?;
        if parts.next().is_some() || user_id.is_empty() || device.is_empty() || name.is_empty() {
            return None;
        }
        // Only accept canonical (already-sanitised) segments, so `parse_key` is a
        // true inverse of `key()` and a non-canonical key (e.g. mixed case) can
        // never be read as a distinct identity by a caller that relies on it.
        if sanitize_component(user_id) != user_id
            || sanitize_component(device) != device
            || sanitize_component(name) != name
        {
            return None;
        }
        Some(Self {
            user_id: user_id.to_string(),
            service: ServiceId {
                device: device.to_string(),
                kind,
                name: name.to_string(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use pocket_codex_core::service::ServiceKind;

    use super::*;

    #[test]
    fn formats_and_parses_round_trip() {
        let id = NamespacedServiceId::new(
            "User-42",
            ServiceId::new("macbook", ServiceKind::App, "work"),
        );
        assert_eq!(id.key(), "pcxu:user-42:macbook:app:work");
        let parsed = NamespacedServiceId::parse_key(&id.key()).expect("parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn user_prefix_scopes_a_listing() {
        assert_eq!(NamespacedServiceId::user_prefix("Bob"), "pcxu:bob:");
        assert!("pcxu:bob:studio:api:default".starts_with(&NamespacedServiceId::user_prefix("bob")));
        assert!(
            !"pcxu:alice:studio:api:default".starts_with(&NamespacedServiceId::user_prefix("bob"))
        );
    }

    #[test]
    fn rejects_self_host_and_malformed_keys() {
        assert!(NamespacedServiceId::parse_key("pcx:studio:api:default").is_none());
        assert!(NamespacedServiceId::parse_key("pcxu:bob:studio:api").is_none());
        assert!(NamespacedServiceId::parse_key("pcxu::studio:app:default").is_none());
    }

    #[test]
    fn key_cannot_escape_the_namespace_via_a_struct_literal_service() {
        // A ServiceId built by struct literal (bypassing ::new) with ':' in its
        // fields must still yield a key with exactly the 4 segment separators,
        // inside this user's prefix — no injected segments, no foreign prefix.
        let id = NamespacedServiceId::new(
            "bob",
            ServiceId {
                device: "x:pcxu:victim:y".to_string(),
                kind: ServiceKind::App,
                name: "n:m".to_string(),
            },
        );
        assert_eq!(id.key().matches(':').count(), 4);
        assert!(id.key().starts_with("pcxu:bob:"));
    }

    #[test]
    fn parse_key_rejects_non_canonical_segments() {
        // Mixed-case / unsanitised segments are not keys the system could emit.
        assert!(NamespacedServiceId::parse_key("pcxu:Bob:dev:app:name").is_none());
        assert!(NamespacedServiceId::parse_key("pcxu:bob:Dev:app:name").is_none());
    }
}
