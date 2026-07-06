//! Timing constants and reconnect backoff for the broker, copied from
//! pb-mapper so both ends share one source of truth.
//!
//! The durations mirror `deps/pb-mapper/src/common/config.rs` (publisher
//! heartbeat / lease / health-probe cadence) so the broker control loop behaves
//! exactly like a pb-mapper publisher — except [`STREAM_DIAL_TIMEOUT`], which
//! is deliberately NOT pb-mapper's 1 s value (that one budgets a loopback dial;
//! a broker data tunnel budgets a WAN TLS handshake).

use std::time::Duration;

/// How often the client sends a heartbeat ping on a register control tunnel.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// How long the client tolerates no inbound control frame before probing
/// liveness out-of-band (= 3 missed heartbeats).
pub const HEARTBEAT_TOLERANCE: Duration = Duration::from_secs(6);

/// Extra grace added to the tolerance before reconnecting on a failed probe.
pub const SUSPECT_GRACE: Duration = Duration::from_secs(2);

/// Timeout of the out-of-band liveness probe.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// Backend read-idle timeout on a register control tunnel: this much silence
/// and the backend retires the registration leg.
pub const LEASE_TIMEOUT: Duration = Duration::from_secs(15);

/// Per read/write timeout on control frames and on the data-tunnel handshake.
pub const CONTROL_IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Subscribe-side health-probe cadence.
pub const HEALTH_INTERVAL: Duration = Duration::from_secs(1);

/// How long the backend keeps a pending loopback accept while waiting for the
/// client to dial back the answering `RegisterData` tunnel.
///
/// NOT pb-mapper's 1 s `STREAM_READY_TIMEOUT`: that budgets a loopback dial,
/// whereas this budgets the client's WAN TLS handshake + a round trip (review
/// finding A1). Generous so a phone on cellular still pairs in time.
pub const STREAM_DIAL_TIMEOUT: Duration = Duration::from_secs(10);

/// Lower bound of the reconnect backoff.
pub const BACKOFF_MIN: Duration = Duration::from_millis(100);

/// Upper bound of the reconnect backoff.
pub const BACKOFF_MAX: Duration = Duration::from_secs(1);

/// Upper bound of the REGISTER-loop reconnect backoff, deliberately much
/// larger than [`BACKOFF_MAX`]: a publisher that cannot hold a session (dead
/// backend, key kept by someone else on an old backend) must degrade to a slow
/// trickle instead of hammering at 1 Hz forever — the 2026-07-07 lb7666 outage
/// was a register storm flooding 24 GB of logs and filling the disk. Normal
/// operation is unaffected: the backoff only grows on consecutive failures and
/// resets once a session proves stable ([`STABLE_SESSION_MIN`]).
pub const REGISTER_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// How long a register session must survive before the reconnect backoff
/// resets. Resetting on mere handshake success is what fueled the duplicate-key
/// leapfrog storm: each contender's handshake "succeeded" for ~100 ms before
/// the other evicted it, so the backoff never grew. A session only counts as
/// healthy once it outlives the takeover churn window.
pub const STABLE_SESSION_MIN: Duration = Duration::from_secs(5);

/// How long the backend parks an id-less (legacy) register hello before
/// rejecting it with a key conflict. Legacy clients reconnect-loop on any
/// rejection with a ≤[`BACKOFF_MAX`] cap, so the tarpit bounds their retry
/// rate; current clients carry an instance id and stop on the first conflict,
/// never hitting this.
pub const CONFLICT_TARPIT: Duration = Duration::from_secs(5);

/// Reconnect backoff: pb-mapper's curve (100 ms → 1 s, base-2) plus optional
/// ±20% jitter.
///
/// pb-mapper itself uses no jitter, but the broker fans many clients into one
/// backend, so a backend restart would otherwise thunder every client at the
/// same 100/200/400 ms steps (review finding G4); callers should prefer
/// [`RetryBackoff::next_delay_jittered`].
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

    /// A fresh backoff with a custom upper bound (e.g.
    /// [`REGISTER_BACKOFF_MAX`] for the register loop). The curve still starts
    /// at [`BACKOFF_MIN`] and doubles per failure.
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
        let mut b = RetryBackoff::with_max(REGISTER_BACKOFF_MAX);
        // Same doubling curve from BACKOFF_MIN...
        assert_eq!(b.next_delay().as_millis(), 100);
        assert_eq!(b.next_delay().as_millis(), 200);
        // ...but it climbs past the default 1 s cap up to the register cap.
        let mut last = 0;
        for _ in 0..12 {
            last = b.next_delay().as_millis();
        }
        assert_eq!(last as u64, REGISTER_BACKOFF_MAX.as_millis() as u64);
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
}
