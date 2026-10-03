//! Restart policy of the agent process supervisor (TRD §4.2.3).
//!
//! ```text
//! Stopped ──start──► Starting ──initialize ok──► Ready
//!    ▲                  │ failure / 60 s             │ exit or EOF
//!    │ stop             ▼                            ▼
//!    └──────────── Restarting(attempt) ◄──────── Crashed
//!                       │ 5th failure within 5 min
//!                       ▼
//!                     Failed (manual restart only)
//! ```

use std::{collections::VecDeque, time::Duration};

use tokio::time::Instant;

/// Failures within [`FAILURE_WINDOW`] that end in `Failed`.
pub const MAX_FAILURES: usize = 5;
/// Window for counting failures.
pub const FAILURE_WINDOW: Duration = Duration::from_secs(300);
/// Upper bound of the restart delay.
pub const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Delay before restart `attempt` (1-based): 1, 2, 4, 8, 16 s, capped at 30 s.
pub fn backoff(attempt: u32) -> Duration {
    let secs = 1u64
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u64::MAX);
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

/// Recent failures of one hub.
#[derive(Debug, Default)]
pub struct FailureLog(VecDeque<Instant>);

impl FailureLog {
    /// Record a failure; returns true when the hub should give up.
    pub fn record(&mut self) -> bool {
        let now = Instant::now();
        self.0.push_back(now);
        while self.0.front().is_some_and(|at| now - *at > FAILURE_WINDOW) {
            self.0.pop_front();
        }
        self.0.len() >= MAX_FAILURES
    }

    /// Failures within the window.
    pub fn recent(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap_or(u32::MAX)
    }

    /// Forget all failures (manual restart).
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let delays: Vec<u64> = (1..=7).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(delays, vec![1, 2, 4, 8, 16, 30, 30]);
    }

    #[tokio::test(start_paused = true)]
    async fn fifth_failure_within_window_gives_up() {
        let mut log = FailureLog::default();
        for _ in 0..4 {
            assert!(!log.record());
        }
        tokio::time::advance(FAILURE_WINDOW + Duration::from_secs(1)).await;
        assert!(!log.record());
        for _ in 0..3 {
            assert!(!log.record());
        }
        assert!(log.record());
    }
}
