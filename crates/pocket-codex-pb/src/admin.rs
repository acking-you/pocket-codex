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
//! holds no copy of, and a key id is the only per-key state. Two properties fall
//! out of that which a shared key could not offer:
//!
//! * **Revocation is immediate, not eventual.** A live credential holds a lease
//!   carrying a cancellation token, so [`revoke_credential`] tears down that
//!   client's in-flight connections rather than merely failing its next attempt.
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
