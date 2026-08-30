//! Administrator operations against a relay: minting and withdrawing the
//! short-lived credentials clients actually use.
//!
//! ```text
//!     backend (holds the admin key)          relay
//!     ─────────────────────────────          ─────────────────────────────
//!     issue_credential(ttl) ───────────────▶ derive from
//!            ◀───── pbmt1_… + key id         (root, instance id, key id)
//!            │                                        │
//!            ▼                                        │
//!     hand to one client                              │
//!            │                                        ▼
//!            └── register/subscribe ────────▶ re-derive and compare
//! ```
//!
//! # Why the relay stores no client secrets
//!
//! A temporary credential is HKDF-derived from the relay's root key, its server
//! instance id, and the key id. The relay can therefore verify a credential it
//! holds no copy of, and a key id is the only per-key state. Two properties
//! fall out of that which a shared key could not offer:
//!
//! * **Revocation is immediate, not eventual.** A live credential holds a lease
//!   carrying a cancellation token, so [`revoke_credential`] tears down that
//!   client's in-flight connections rather than merely failing its next
//!   attempt.
//! * **Everything can be invalidated without touching individual keys**, by
//!   rotating the root or resetting the instance id — both inputs to the
//!   derivation.
//!
//! # The one rule
//!
//! Only the administrator key may call anything here, and it must never leave
//! the process that holds it. Hand out [`IssuedCredential::credential`]; never
//! the admin key itself. The SDK enforces the first half locally — asking an
//! admin RPC of a temporary-credential session fails before any I/O — but only
//! the caller can enforce the second.

use std::time::Duration;

use anyhow::{Context, Result};

use crate::session::RelaySession;

/// A credential the relay just minted, as handed to exactly one client.
///
/// The relay returns key material only at issue time — it will not repeat a
/// secret it has already delivered — so [`Self::credential`] must be captured
/// here or re-issued.
#[derive(Debug, Clone)]
pub struct IssuedCredential {
    /// The `pbmt1_…` string the client presents to the relay.
    pub credential: String,
    /// The relay's handle for this credential, for renew and revoke. Also the
    /// namespace the credential is confined to.
    pub key_id: u64,
    /// Unix seconds at which the relay stops accepting it.
    pub expires_at: u64,
}

/// Mint a temporary credential valid for `ttl`.
///
/// `session` must present the administrator key. `label` is opaque to the relay
/// and exists so an operator listing credentials can tell which client a key
/// belongs to — pass something identifying, like the account and device.
pub async fn issue_credential(
    session: &RelaySession,
    ttl: Duration,
    label: Option<String>,
) -> Result<IssuedCredential> {
    let issued = session
        .admin()?
        .issue_key(ttl, label)
        .await
        .with_context(|| format!("issuing a temporary credential on {}", session.relay_addr))?;
    Ok(IssuedCredential {
        credential: issued.credential,
        key_id: issued.key_id,
        expires_at: issued.expires_at,
    })
}

/// Extend an existing credential's life, keeping its key id.
///
/// Preferred over issuing a replacement whenever the old credential is still
/// alive: the key id IS the namespace, so a fresh credential would move the
/// holder to a new namespace and orphan every service already registered under
/// the old one. Fails once the credential is past renewal, at which point
/// [`issue_credential`] is the only way back.
pub async fn renew_credential(
    session: &RelaySession,
    key_id: u64,
    ttl: Duration,
) -> Result<IssuedCredential> {
    let renewed = session
        .admin()?
        .renew_key(key_id, ttl)
        .await
        .with_context(|| format!("renewing credential {key_id} on {}", session.relay_addr))?;
    Ok(IssuedCredential {
        credential: renewed.credential,
        key_id: renewed.key_id,
        expires_at: renewed.expires_at,
    })
}

/// A credential the relay still holds, without its secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRecord {
    /// The relay's handle for it — and the namespace it is confined to.
    pub key_id: u64,
    /// The label it was issued with, if any.
    pub label: Option<String>,
    /// Unix seconds at which the relay stops accepting it.
    pub expires_at: u64,
}

/// Every credential the relay currently accepts.
///
/// Filtered to live keys: the table retains expired and revoked ones until a
/// sweep, and adopting one of those would hand a caller a namespace nothing can
/// authenticate into.
pub async fn live_credentials(session: &RelaySession) -> Result<Vec<CredentialRecord>> {
    let keys = session
        .admin()?
        .list_keys_all()
        .await
        .with_context(|| format!("listing credentials on {}", session.relay_addr))?;
    Ok(keys
        .into_iter()
        .filter(|key| key.state == "active")
        .map(|key| CredentialRecord {
            key_id: key.key_id,
            label: key.label,
            expires_at: key.expires_at,
        })
        .collect())
}

/// One registered service, as an administrator sees it.
///
/// Carries the owning `namespace` because the service NAME alone cannot be
/// trusted for attribution: a name is whatever its registrant typed, and every
/// namespace has its own. Account B can register the literal string
/// `pcxu:<account-A>:mac:app:default` inside B's namespace — the relay is right
/// to allow it, since namespaces are what isolate them — so a caller matching
/// on the name alone would show B's service as A's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecord {
    /// The relay namespace that owns the registration.
    pub namespace: u64,
    /// The service key the registrant chose.
    pub name: String,
}

/// Every service the relay knows, across all namespaces, with its owner.
///
/// Admin-scoped deliberately: a per-account credential sees only its own
/// namespace, which is right for a client but not for a backend answering "what
/// does this account have registered". That caller must filter on BOTH the
/// namespace and the key prefix — see [`ServiceRecord`] for why the name alone
/// is not enough.
pub async fn all_services(session: &RelaySession) -> Result<Vec<ServiceRecord>> {
    let services = session
        .admin()?
        .list_services_all(None)
        .await
        .with_context(|| format!("listing services on {}", session.relay_addr))?;
    Ok(services
        .into_iter()
        .map(|svc| ServiceRecord {
            namespace: svc.namespace,
            name: svc.service_name,
        })
        .collect())
}

/// Drop the relay's connections for a service in `namespace`.
///
/// For a registration whose owner is gone: the relay still holds the key and
/// the client that would have released it no longer exists. A live client
/// reconnects afterwards, so the cost is a brief interruption at worst.
///
/// `namespace` is REQUIRED, and is the account's, not the administrator's.
/// Omitting it does not mean "wherever this service lives" — the relay reads an
/// absent namespace as the unscoped one, where an administrator's own
/// registrations sit. Retiring an account's service without naming its
/// namespace therefore silently retires nothing.
pub async fn retire_service(
    session: &RelaySession,
    namespace: u64,
    service_name: &str,
) -> Result<u32> {
    session
        .admin()?
        .retire_connections(Some(namespace), service_name.to_string(), None)
        .await
        .with_context(|| {
            format!("retiring `{service_name}` in namespace {namespace} on {}", session.relay_addr)
        })
}

/// Withdraw a credential, cancelling whatever it currently holds open.
///
/// Idempotent from the caller's point of view in the way that matters: revoking
/// an already-dead key is not an error worth distinguishing, because the
/// post-condition — that key cannot authenticate — holds either way.
pub async fn revoke_credential(session: &RelaySession, key_id: u64) -> Result<()> {
    session
        .admin()?
        .revoke_key(key_id)
        .await
        .map(|_| ())
        .with_context(|| format!("revoking credential {key_id} on {}", session.relay_addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_temporary_credential_cannot_drive_admin_rpcs() {
        // The privilege split is what the whole design rests on, and the SDK
        // checks it locally — so this holds without a relay to talk to.
        let session = RelaySession::new(
            "127.0.0.1:7666",
            "pbmt1_AQAAAAEAAAAAFNsL8qDUIBWtvMkHusQ9A6kEm0APq_BDwMZTCuoBCzqAsdCp",
        );
        let err = issue_credential(&session, Duration::from_secs(60), None)
            .await
            .expect_err("a temporary credential must not be able to mint credentials");
        assert!(
            format!("{err:#}").to_lowercase().contains("administrator"),
            "expected an administrator-privilege error, got: {err:#}"
        );
    }
}
