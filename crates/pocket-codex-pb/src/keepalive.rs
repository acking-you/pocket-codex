//! Keeping a temporary relay credential alive for as long as a tunnel needs it.
//!
//! ```text
//!   issued ────────────────────────────────────────────────▶ expires_at
//!            │                          │
//!            └── refresh ───────────────┴── refresh ──▶ …
//!                (well before expiry, which RENEWS the same credential)
//! ```
//!
//! # Why a long-lived client needs this
//!
//! A temporary credential's expiry is not merely a refusal of the NEXT request:
//! the relay holds a lease per credential, and expiry cancels it, tearing down
//! every tunnel that credential opened. A host that registered once and then
//! went quiet would therefore stop serving at its credential's TTL, without
//! anything having gone wrong.
//!
//! Renewal moves the existing lease's expiry rather than replacing it, and
//! hands back the SAME credential string — so a client that renews in time
//! keeps both its live tunnels and the value it is holding. That is what makes
//! a periodic refresh sufficient, and why nothing here has to hand a new
//! credential back to its caller.

use std::{future::Future, time::Duration};

use tokio::task::JoinHandle;

/// How long before expiry to refresh.
///
/// Generous relative to any sane TTL: the cost of refreshing early is one
/// request, and the cost of refreshing late is every tunnel this credential
/// holds. It also has to cover a device that was asleep across the deadline.
const REFRESH_MARGIN: Duration = Duration::from_secs(30 * 60);

/// Never sleep longer than this between refreshes, however distant the expiry.
///
/// A credential can be renewed to an expiry far in the future, and a single
/// multi-day sleep would make one lost request cost a whole outage. Waking
/// regularly is cheap and means a transient failure has many chances to
/// recover.
const MAX_SLEEP: Duration = Duration::from_secs(6 * 60 * 60);

/// Retry delay after a failed refresh — the backend may be briefly down, and
/// the margin above leaves room for several attempts before anything breaks.
const RETRY_DELAY: Duration = Duration::from_secs(60);

/// Refresh this credential in the background for as long as the returned handle
/// (or the process) lives.
///
/// `expires_at` is the current expiry in unix seconds, and `refresh` is
/// whatever asks the issuer to renew — for a hosted account, a `GET /v1/relay`
/// — returning the new expiry. The credential itself is not returned because
/// renewal does not change it.
///
/// Dropping the handle stops refreshing; it does not stop the tunnels, which
/// keep working until the credential actually lapses.
pub fn keep_credential_alive<F, Fut>(expires_at: u64, refresh: F) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<u64>> + Send,
{
    tokio::spawn(async move {
        let mut expires_at = expires_at;
        loop {
            tokio::time::sleep(sleep_until_refresh(expires_at, now_secs())).await;
            match refresh().await {
                Ok(next) if next > expires_at => expires_at = next,
                Ok(next) => {
                    // The issuer would not extend it. Nothing here can fix that,
                    // and retrying in a tight loop would only add noise — so keep
                    // the schedule and let the next attempt try again.
                    tracing::warn!(
                        expires_at,
                        returned = next,
                        "the relay credential was not extended; tunnels will drop at expiry"
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                },
                Err(err) => {
                    tracing::warn!(
                        error = %format!("{err:#}"),
                        expires_at,
                        "refreshing the relay credential failed; will retry"
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                },
            }
        }
    })
}

/// How long to wait before the next refresh attempt.
///
/// Factored out so the schedule is testable without waiting on a clock: a
/// credential already inside its margin (or past expiry) refreshes immediately,
/// and one with plenty of life left still wakes within [`MAX_SLEEP`].
fn sleep_until_refresh(expires_at: u64, now: u64) -> Duration {
    let refresh_at = expires_at.saturating_sub(REFRESH_MARGIN.as_secs());
    Duration::from_secs(refresh_at.saturating_sub(now)).min(MAX_SLEEP)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_immediately_once_inside_the_margin() {
        let now = 1_000_000;
        // Expiry is closer than the margin: there is no time left to wait for.
        assert_eq!(sleep_until_refresh(now + 60, now), Duration::ZERO);
        // Already expired — still zero rather than a wrapped, enormous sleep,
        // which is the bug `saturating_sub` is here to prevent.
        assert_eq!(sleep_until_refresh(now - 5_000, now), Duration::ZERO);
    }

    #[test]
    fn waits_until_the_margin_for_a_credential_with_life_left() {
        let now = 1_000_000;
        let ttl = 2 * 60 * 60; // 2h, comfortably beyond the 30m margin
        assert_eq!(
            sleep_until_refresh(now + ttl, now),
            Duration::from_secs(ttl - REFRESH_MARGIN.as_secs())
        );
    }

    #[test]
    fn never_sleeps_past_the_cap() {
        let now = 1_000_000;
        // A year out: one uninterrupted sleep would make a single lost refresh a
        // total outage, so the schedule caps the wait instead.
        let sleep = sleep_until_refresh(now + 365 * 24 * 60 * 60, now);
        assert_eq!(sleep, MAX_SLEEP);
    }
}
