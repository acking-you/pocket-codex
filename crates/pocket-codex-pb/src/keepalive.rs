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
/// Call `abort()` on the returned task to stop refreshing. As with any Tokio
/// `JoinHandle`, dropping it detaches the task; it does not stop the tunnels.
pub fn keep_credential_alive<F, Fut>(expires_at: u64, refresh: F) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<u64>> + Send,
{
    tokio::spawn(async move {
        let mut expires_at = expires_at;
        let mut delay = sleep_until_refresh(expires_at, now_secs());
        loop {
            tokio::time::sleep(delay).await;
            match refresh().await {
                Ok(next) => {
                    let now = now_secs();
                    if next <= now.saturating_add(REFRESH_MARGIN.as_secs()) {
                        tracing::warn!(
                            expires_at = next,
                            "relay credential remains near expiry; will retry"
                        );
                    } else {
                        tracing::debug!(expires_at = next, "relay credential remains valid");
                    }
                    // Issuers may return an unchanged deadline outside their renewal
                    // window, or shorten it. Always schedule from the returned value.
                    expires_at = next;
                    delay = sleep_until_refresh(expires_at, now).max(RETRY_DELAY);
                },
                Err(err) => {
                    tracing::warn!(
                        error = %format!("{err:#}"),
                        expires_at,
                        "refreshing the relay credential failed; will retry"
                    );
                    delay = RETRY_DELAY;
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

    #[tokio::test(start_paused = true)]
    async fn failed_refresh_retries_after_one_minute_without_another_long_sleep() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&attempts);
        let task = keep_credential_alive(now_secs() + 24 * 60 * 60, move || {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("temporary issuer outage")
            }
        });
        tokio::task::yield_now().await;
        tokio::time::advance(MAX_SLEEP).await;
        tokio::task::yield_now().await;
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        tokio::time::advance(RETRY_DELAY).await;
        tokio::task::yield_now().await;
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        task.abort();
    }

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
