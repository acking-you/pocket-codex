//! Per-account relay credentials: minted under the backend's administrator key,
//! handed to clients so they can reach the relay without the backend in the
//! way.
//!
//! ```text
//!   client ──JWT──▶ backend ──admin key──▶ relay      (once per TTL)
//!          ◀── pbmt1_… ──┘
//!   client ─────────── pbmt1_… ───────────▶ relay      (every byte after)
//! ```
//!
//! # One credential per account
//!
//! A temporary credential's namespace IS its key id, and namespaces are hard
//! isolation. Minting per DEVICE would put an account's phone and desktop in
//! different namespaces, unable to see each other's services — which is the
//! product. So every device on an account gets the SAME credential, and the
//! isolation boundary sits between accounts, where it belongs.
//!
//! # Why cache
//!
//! The relay's key table is finite (65,536 slots by default) and every issue
//! consumes one until it expires. Minting per request would exhaust it under
//! ordinary polling, so an account's credential is minted once, renewed while
//! it is still valid, and only re-minted once it cannot be.

use std::{collections::HashMap, sync::Arc, time::Duration};

use pocket_codex_pb::RelaySession;
use tokio::sync::Mutex;

/// How long an issued credential lives.
///
/// Long enough that a device is not re-asking constantly, short enough that a
/// credential leaked off a lost phone stops working on its own. Revocation
/// exists for the urgent case; this bounds the unnoticed one.
const CREDENTIAL_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Re-issue when less than this remains, rather than at expiry.
///
/// A client that fetched a credential moments before it lapsed would otherwise
/// get one that dies mid-session. The margin is generous relative to the TTL
/// because the cost of refreshing early is one relay round trip.
const RENEW_MARGIN: Duration = Duration::from_secs(60 * 60);

/// A credential as handed to clients, plus what the backend needs to renew it.
#[derive(Clone)]
struct Cached {
    credential: String,
    key_id: u64,
    expires_at: u64,
}

/// Mints and caches one relay credential per account.
#[derive(Clone)]
pub struct Credentials {
    relay: RelaySession,
    /// Keyed by internal user id. A mutex rather than a lock-free map because
    /// the contended path is a relay round trip, and holding the lock
    /// across it is what stops a thundering herd of first-time requests
    /// from minting one credential each.
    cache: Arc<Mutex<HashMap<String, Cached>>>,
}

impl Credentials {
    /// Build a vendor that mints under `relay`'s credential, which must be the
    /// relay's administrator key.
    pub fn new(relay: RelaySession) -> Self {
        Self {
            relay,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The relay address clients should dial.
    pub fn relay_addr(&self) -> &str {
        &self.relay.relay_addr
    }

    /// This account's credential, minting or renewing it if needed.
    ///
    /// Returns `(credential, expires_at)`.
    pub async fn for_account(&self, user_id: &str) -> anyhow::Result<(String, u64)> {
        let mut cache = self.cache.lock().await;
        let now = now_secs();
        if let Some(cached) = cache.get(user_id) {
            if cached.expires_at > now + RENEW_MARGIN.as_secs() {
                return Ok((cached.credential.clone(), cached.expires_at));
            }
            // Still known to the relay: renewing keeps the SAME key id, so the
            // account keeps its namespace and every service already registered
            // under it stays addressable. Re-minting would move the account to a
            // new namespace and orphan its live registrations.
            match pocket_codex_pb::renew_credential(&self.relay, cached.key_id, CREDENTIAL_TTL)
                .await
            {
                Ok(renewed) => {
                    let entry = Cached {
                        credential: renewed.credential,
                        key_id: renewed.key_id,
                        expires_at: renewed.expires_at,
                    };
                    cache.insert(user_id.to_string(), entry.clone());
                    return Ok((entry.credential, entry.expires_at));
                },
                Err(err) => {
                    // Expired past renewal, revoked, or lost to a state reset.
                    // Falling through to a fresh mint is the only way back, and
                    // it is worth a log line because the account's namespace
                    // changes with it.
                    tracing::warn!(
                        user = %user_id,
                        key_id = cached.key_id,
                        error = %format!("{err:#}"),
                        "renewing the relay credential failed; minting a new one"
                    );
                },
            }
        }
        let issued = pocket_codex_pb::issue_credential(
            &self.relay,
            CREDENTIAL_TTL,
            // Labelled with the account so an operator listing the relay's keys
            // can tell whose a credential is without consulting the database.
            Some(format!("pcx-account:{user_id}")),
        )
        .await?;
        let entry = Cached {
            credential: issued.credential,
            key_id: issued.key_id,
            expires_at: issued.expires_at,
        };
        cache.insert(user_id.to_string(), entry.clone());
        Ok((entry.credential, entry.expires_at))
    }

    /// Withdraw this account's credential, cutting its devices off the relay.
    ///
    /// Best-effort by design: the post-condition a caller wants is "that
    /// credential no longer works", and a credential the relay has already
    /// forgotten satisfies it. Dropping the cache entry is the part that must
    /// not be skipped, so it happens regardless.
    pub async fn revoke_account(&self, user_id: &str) {
        let cached = self.cache.lock().await.remove(user_id);
        let Some(cached) = cached else { return };
        if let Err(err) = pocket_codex_pb::revoke_credential(&self.relay, cached.key_id).await {
            tracing::warn!(
                user = %user_id,
                key_id = cached.key_id,
                error = %format!("{err:#}"),
                "revoking the relay credential failed"
            );
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
