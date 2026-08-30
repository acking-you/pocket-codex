//! Retry pacing for request-shaped work, on pb-mapper's backoff curve.
//!
//! This module used to hold the whole broker control loop's timing — heartbeat
//! cadence, lease timeout, stream-dial budget, the register-loop backoff tuned
//! after the 2026-07-07 register storm. All of that belonged to a tunnel
//! protocol we no longer implement: the pb-mapper SDK owns reconnecting now, so
//! the constants that paced OUR reconnect loop went with it.
//!
//! What is left is the bounded retry a user-facing request needs — see
//! [`BoundedRetry`] — plus the curve it waits on.

use std::time::Duration;

/// Lower bound of the retry backoff.
pub const BACKOFF_MIN: Duration = Duration::from_millis(100);

/// Upper bound of the retry backoff.
pub const BACKOFF_MAX: Duration = Duration::from_secs(1);

/// Retry backoff: pb-mapper's curve (100 ms → 1 s, base-2) plus optional ±20%
/// jitter.
///
/// pb-mapper itself uses no jitter, but many clients converge on one host, so a
/// host restart would otherwise thunder every client at the same 100/200/400 ms
/// steps; callers should prefer [`RetryBackoff::next_delay_jittered`].
#[derive(Debug, Clone)]
pub struct RetryBackoff {
    failures: u32,
    max: Duration,
}

impl Default for RetryBackoff {
    fn default() -> Self {
        Self {
            failures: 0,
            max: BACKOFF_MAX,
        }
    }
}

impl RetryBackoff {
    /// A fresh backoff (no failures recorded yet), capped at [`BACKOFF_MAX`].
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh backoff with a custom upper bound, for a caller whose work is
    /// slower or cheaper than the default assumes. The curve still starts at
    /// [`BACKOFF_MIN`] and doubles per failure.
    pub fn with_max(max: Duration) -> Self {
        Self {
            failures: 0,
            max,
        }
    }

    /// Record a failure and return the next base delay (no jitter).
    pub fn next_delay(&mut self) -> Duration {
        let shift = self.failures.min(10);
        let min_ms = BACKOFF_MIN.as_millis() as u64;
        let max_ms = self.max.as_millis() as u64;
        let millis = (min_ms << shift).min(max_ms);
        self.failures = self.failures.saturating_add(1);
        Duration::from_millis(millis)
    }

    /// Like [`Self::next_delay`] but applies ±20% jitter using a
    /// caller-supplied uniform `sample` in `[0, 1)` (so this crate needs no
    /// RNG dependency).
    pub fn next_delay_jittered(&mut self, sample: f64) -> Duration {
        let base = self.next_delay().as_millis() as f64;
        let factor = 0.8 + 0.4 * sample.clamp(0.0, 1.0);
        Duration::from_millis((base * factor) as u64)
    }

    /// Reset after a successful (re)connect, so the next failure starts again
    /// at [`BACKOFF_MIN`].
    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

/// How many times a *bounded* operation is attempted before giving up.
///
/// Ten total attempts (one try + nine retries). Deliberately NOT applied to
/// publishing or supervisor loops: those must retry forever (a host that stops
/// re-publishing is unreachable until the user notices), and their pacing is
/// the pb-mapper SDK's. This bound is for *request-shaped* work, where there is
/// a caller waiting on an answer and "still trying" eventually has to become
/// "it failed".
pub const MAX_ATTEMPTS: u32 = 10;

/// Drives a bounded retry: how long to wait before the next attempt, or
/// `None` once the attempt budget is spent.
///
/// Pairs with [`RetryBackoff`] for the delay curve and adds the two things a
/// user-facing retry needs that a supervisor loop does not: a hard attempt cap,
/// and an attempt counter the caller can report so the wait is legible rather
/// than looking like a hang.
///
/// Retryability is the CALLER's decision, not this type's — only the caller
/// knows whether its request is idempotent. Re-issuing a GET is free;
/// re-issuing a POST that allocates a fresh upload directory duplicates the
/// file.
#[derive(Debug, Clone)]
pub struct BoundedRetry {
    backoff: RetryBackoff,
    attempt: u32,
    max_attempts: u32,
}

impl Default for BoundedRetry {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundedRetry {
    /// A budget of [`MAX_ATTEMPTS`] attempts on the default backoff curve.
    pub fn new() -> Self {
        Self {
            backoff: RetryBackoff::new(),
            attempt: 0,
            max_attempts: MAX_ATTEMPTS,
        }
    }

    /// A budget with custom bounds — for a caller whose work is slower or
    /// cheaper than the default assumes.
    pub fn with_bounds(max_attempts: u32, max_delay: Duration) -> Self {
        Self {
            backoff: RetryBackoff::with_max(max_delay),
            attempt: 0,
            max_attempts,
        }
    }

    /// Attempts made so far (0 before the first).
    pub fn attempts(&self) -> u32 {
        self.attempt
    }

    /// The total attempt budget, for progress reporting ("2 / 10").
    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    /// Record that the attempt just made failed, and return how long to wait
    /// before the next one — or `None` when the budget is exhausted and the
    /// caller should surface the error.
    pub fn fail(&mut self) -> Option<Duration> {
        self.attempt = self.attempt.saturating_add(1);
        if self.attempt >= self.max_attempts {
            return None;
        }
        Some(self.backoff.next_delay())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_follows_pb_mapper_curve_and_caps() {
        let mut b = RetryBackoff::new();
        let seq: Vec<u64> = (0..6).map(|_| b.next_delay().as_millis() as u64).collect();
        assert_eq!(seq, vec![100, 200, 400, 800, 1000, 1000]);
        b.reset();
        assert_eq!(b.next_delay().as_millis() as u64, 100);
    }

    #[test]
    fn with_max_keeps_the_curve_but_raises_the_cap() {
        let raised = Duration::from_secs(30);
        let mut b = RetryBackoff::with_max(raised);
        // Same doubling curve from BACKOFF_MIN...
        assert_eq!(b.next_delay().as_millis(), 100);
        assert_eq!(b.next_delay().as_millis(), 200);
        // ...but it climbs past the default 1 s cap up to the raised one.
        let mut last = 0;
        for _ in 0..12 {
            last = b.next_delay().as_millis();
        }
        assert_eq!(last as u64, raised.as_millis() as u64);
        b.reset();
        assert_eq!(b.next_delay().as_millis(), 100);
    }

    #[test]
    fn jitter_stays_within_20_percent() {
        let mut b = RetryBackoff::new();
        let lo = b.clone().next_delay_jittered(0.0).as_millis();
        let hi = b.next_delay_jittered(1.0).as_millis();
        // base 100ms → ~[80, 120] ms (float-safe ranges).
        assert!((79..=81).contains(&lo), "lo={lo}");
        assert!((118..=121).contains(&hi), "hi={hi}");
    }

    #[test]
    fn bounded_retry_spends_exactly_max_attempts_then_gives_up() {
        let mut r = BoundedRetry::new();
        // Nine waits between ten attempts, then the budget is spent. Returning
        // None on the LAST failure (not after an extra wait) is what stops the
        // UI sitting through a final pointless delay before the error appears.
        let mut waits = 0;
        while r.fail().is_some() {
            waits += 1;
            assert!(waits < MAX_ATTEMPTS, "must terminate");
        }
        assert_eq!(waits, MAX_ATTEMPTS - 1);
        assert_eq!(r.attempts(), MAX_ATTEMPTS);
        assert_eq!(r.max_attempts(), MAX_ATTEMPTS);
        // Still exhausted once past the cap — no wrap-around into a fresh budget.
        assert!(r.fail().is_none());
    }

    #[test]
    fn bounded_retry_follows_the_backoff_curve_and_reports_progress() {
        let mut r = BoundedRetry::new();
        assert_eq!(r.attempts(), 0);
        assert_eq!(r.fail().map(|d| d.as_millis()), Some(100));
        assert_eq!(r.attempts(), 1); // legible as "1 / 10"
        assert_eq!(r.fail().map(|d| d.as_millis()), Some(200));
        assert_eq!(r.fail().map(|d| d.as_millis()), Some(400));
    }

    #[test]
    fn bounded_retry_honours_custom_bounds() {
        let mut r = BoundedRetry::with_bounds(2, Duration::from_millis(150));
        assert_eq!(r.fail().map(|d| d.as_millis()), Some(100));
        assert!(r.fail().is_none()); // two attempts, one wait
                                     // A single-attempt budget never waits at all.
        let mut once = BoundedRetry::with_bounds(1, BACKOFF_MAX);
        assert!(once.fail().is_none());
    }
}
