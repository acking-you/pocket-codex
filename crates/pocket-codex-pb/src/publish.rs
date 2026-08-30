//! Publishing a local service and keeping it published.
//!
//! ```text
//!   publish(session, opts) ──▶ Published { handle }
//!         │                        │
//!         │                        ├── status(): Starting | Connected |
//!         │                        │             Retrying | Failed(why) | Stopped
//!         │                        └── stop(): release the key now
//!         ▼
//!   Err(PublishError::Conflict) when the name is already live elsewhere
//! ```
//!
//! # Why this is thin
//!
//! Reconnecting a dropped registration used to be ours: a loop that redialled
//! with backoff, decided which failures were permanent, and reported a name
//! conflict back to the caller — several hundred lines, and the source of the
//! duplicate-name register storm when it misread a permanent refusal as a
//! transport blip. The SDK does the reconnecting now, and distinguishes a
//! retryable remote error from a permanent one on the wire. So all that is left
//! here is to name the one distinction callers act on:
//!
//! * **Conflict** — another live instance owns this name. Retrying cannot fix
//!   it (and retrying anyway IS the storm), so hosting must fail and say so.
//! * **Everything else** — the relay is unreachable or unhappy for now. Hold
//!   the handle; the SDK keeps trying.

use anyhow::Result;
use pb_mapper::{Registration, TunnelStatus};

use crate::session::{RegisterOptions, RelaySession};

/// Why a publish attempt did not result in a live registration.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    /// The name is already published by another live instance.
    ///
    /// Distinguished from every other failure because it is the one a caller
    /// must not retry: two publishers of one key evict each other on the
    /// relay in an endless leapfrog, which is what took production down on
    /// 2026-07-07.
    #[error("`{key}` is already registered and online — another device or process owns this name")]
    Conflict {
        /// The contested service key.
        key: String,
        /// The relay's own words, kept for the log.
        detail: String,
    },
    /// Anything else: the relay is unreachable, over quota, or refusing for a
    /// reason that may clear on its own.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// A live registration. Dropping it stops publishing.
///
/// `Debug` reports the key and current status, never anything about the
/// credential that opened the tunnel.
pub struct Published {
    registration: Registration,
}

impl std::fmt::Debug for Published {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Published")
            .field("key", &self.registration.key())
            .field("status", &self.registration.status())
            .finish()
    }
}

impl Published {
    /// Whether the tunnel is up right now.
    ///
    /// `Retrying` reads as NOT published: the key may still be indexed, but
    /// nothing is forwarding, which is the state a UI needs to show as degraded
    /// rather than green.
    pub fn is_live(&self) -> bool {
        matches!(self.registration.status(), TunnelStatus::Connected)
    }

    /// Why the tunnel is down, if the relay gave a permanent reason.
    ///
    /// `None` while it is connected or still retrying — a retry in progress is
    /// not a failure to report.
    pub fn failure(&self) -> Option<String> {
        match self.registration.status() {
            TunnelStatus::Failed(reason) => Some(reason),
            _ => None,
        }
    }

    /// Unpublish, awaiting the teardown.
    ///
    /// Preferred over dropping: the relay frees the key now rather than at its
    /// next lease sweep, so an immediate re-publish under the same name does
    /// not collide with the registration just abandoned.
    pub async fn stop(self) -> Result<()> {
        self.registration
            .stop()
            .await
            .map_err(|err| anyhow::anyhow!("stopping the registration: {err}"))
    }
}

/// Publish `opts.local_addr` on the relay under `opts.key`.
///
/// The returned handle owns the registration — hold it for as long as the
/// service should stay published. Transient relay trouble is the SDK's to
/// retry; only a name conflict comes back as [`PublishError::Conflict`].
///
/// Succeeds even when the relay has not CONFIRMED the registration yet, because
/// the handle's worker is still reconnecting and holding it is what lets the
/// service come up once the relay is reachable. Use [`publish_pending`] when
/// that distinction matters to the caller.
pub async fn publish(
    session: &RelaySession,
    opts: RegisterOptions,
) -> Result<Published, PublishError> {
    publish_pending(session, opts)
        .await
        .map(|(published, _)| published)
}

/// Publish, and say whether it is up YET.
///
/// `Ok((handle, false))` means the relay has not confirmed the registration
/// within the SDK's readiness budget. That is not a failure: the worker keeps
/// retrying, so a caller that holds the handle gets the service published once
/// the relay is back — and a caller that needs to report status has the flag to
/// do it with.
///
/// This shape exists because the obvious alternative is a trap: wrapping a
/// publish in `tokio::time::timeout` CANCELS it, dropping the handle and with
/// it the retrying worker, so a slow relay would leave the service permanently
/// unpublished rather than merely late.
pub async fn publish_pending(
    session: &RelaySession,
    opts: RegisterOptions,
) -> Result<(Published, bool), PublishError> {
    let key = opts.key.clone();
    match crate::session::register_pending(session, opts).await {
        Ok((registration, ready)) => {
            if let Err(err) = &ready {
                tracing::warn!(
                    key = %key,
                    error = %format!("{err:#}"),
                    "the relay has not confirmed this registration yet; it keeps retrying"
                );
            }
            Ok((
                Published {
                    registration,
                },
                ready.is_ok(),
            ))
        },
        Err(err) => {
            let detail = format!("{err:#}");
            if is_conflict(&detail) {
                return Err(PublishError::Conflict {
                    key,
                    detail,
                });
            }
            Err(PublishError::Other(err))
        },
    }
}

/// Whether a register failure means "this name is taken", by the relay's own
/// error code.
///
/// String-matched because the SDK flattens a remote rejection's code into the
/// error's display text rather than exposing it as a field. That is a fragile
/// seam, so it is isolated here and errs toward NOT claiming a conflict: a
/// misread conflict would fail a host that could have served, whereas a missed
/// one degrades to the pre-existing behaviour of holding the handle and
/// retrying.
fn is_conflict(detail: &str) -> bool {
    let detail = detail.to_lowercase();
    ["service_already_registered", "already registered", "key_conflict", "duplicate_service"]
        .iter()
        .any(|marker| detail.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_relay_conflict_codes() {
        assert!(is_conflict("registering `pcx:a:app:b` on relay:7666: service_already_registered"));
        assert!(is_conflict("SERVICE_ALREADY_REGISTERED: name in use"));
        assert!(is_conflict("key_conflict: another publisher holds this key"));
    }

    #[test]
    fn does_not_read_a_transient_failure_as_a_conflict() {
        // These must keep retrying. Calling any of them a conflict would fail a
        // host that is merely waiting for a relay to come back.
        assert!(!is_conflict("connect to `relay:7666` failed: connection refused"));
        assert!(!is_conflict("timed out waiting for the tunnel to become ready after 30s"));
        assert!(!is_conflict("service_connection_limit_exceeded"));
    }

    #[tokio::test]
    async fn an_unreachable_relay_keeps_a_retrying_handle_rather_than_failing() {
        // Port 1 on loopback refuses immediately, so this is the real "relay is
        // down" path without a relay. It must NOT be an error: the SDK's worker is
        // reconnecting, and the handle is what lets the service come up on its own
        // once the relay is back. Failing here — or dropping the handle — is what
        // would leave a host permanently unpublished after a blip.
        let session = RelaySession::for_test("127.0.0.1:1");
        let (published, ready) = publish_pending(&session, RegisterOptions {
            key: "pcx:test:app:default".to_string(),
            local_addr: "127.0.0.1:9".to_string(),
            codec: false,
        })
        .await
        .expect("an unreachable relay is a pending publish, not a failed one");
        assert!(!ready, "nothing is listening, so this cannot be ready");
        assert!(!published.is_live(), "a retrying tunnel must not read as live");
        // And not as a permanent failure either: `failure()` is what
        // `needs_publishing` uses to decide whether to publish AGAIN, and a second
        // registration for one key is the eviction leapfrog.
        assert_eq!(
            published.failure(),
            None,
            "a retrying tunnel must not look permanently refused"
        );
    }

    #[tokio::test]
    async fn a_publish_to_nowhere_is_not_reported_as_a_conflict() {
        // `publish` treats not-ready as success for the same reason: the caller
        // holds the handle, so the retry survives. What it must never do is call an
        // unreachable relay a name conflict, which would permanently fail hosting.
        let session = RelaySession::for_test("127.0.0.1:1");
        let published = publish(&session, RegisterOptions {
            key: "pcx:test:app:default".to_string(),
            local_addr: "127.0.0.1:9".to_string(),
            codec: false,
        })
        .await
        .expect("an unreachable relay must not be reported as a conflict");
        assert!(!published.is_live());
    }
}
