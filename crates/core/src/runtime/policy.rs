use std::{
    collections::VecDeque,
    hash::{BuildHasher, Hasher},
    time::{Duration, Instant},
};

/// When and how often a crashed connector is restarted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPolicy {
    /// First backoff.
    pub initial_backoff: Duration,
    /// Backoff cap.
    pub max_backoff: Duration,
    /// Crashes within `crash_window` that count as a crash loop.
    pub crash_loop_threshold: usize,
    /// Window for crash-loop detection.
    pub crash_window: Duration,
    /// How long `Connecting` may last before it counts as `Degraded`.
    pub connect_timeout: Duration,
    /// `/ready` polling interval.
    pub health_interval: Duration,
    /// Grace period between SIGTERM and SIGKILL.
    pub stop_grace: Duration,
}

impl Default for RestartPolicy {
    /// ARCHITECTURE §5.1: 1 s → 60 s backoff; more than 5 crashes in 2 min is a loop.
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            crash_loop_threshold: 5,
            crash_window: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(30),
            health_interval: Duration::from_secs(2),
            stop_grace: Duration::from_secs(5),
        }
    }
}

impl RestartPolicy {
    /// Exponential backoff for `attempt` (1-based), capped, with ±20% jitter so many
    /// connectors don't restart in lockstep.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        let base = self
            .initial_backoff
            .saturating_mul(1 << exponent)
            .min(self.max_backoff);
        let jitter = jitter_fraction(); // 0.8 ..= 1.2
        base.mul_f64(jitter).min(self.max_backoff)
    }
}

/// A random factor in 0.8..=1.2 without an RNG dependency (std's hasher is randomly
/// seeded per process).
fn jitter_fraction() -> f64 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(Instant::now().elapsed().as_nanos());
    let unit = (hasher.finish() % 10_000) as f64 / 10_000.0;
    0.8 + unit * 0.4
}

/// Remembers recent crashes to detect crash loops.
#[derive(Debug, Default)]
pub struct CrashTracker {
    crashes: VecDeque<Instant>,
}

impl CrashTracker {
    /// Records a crash at `now`; returns `(attempt, is_crash_loop)`, where `attempt` is
    /// the number of crashes inside the window.
    pub fn record(&mut self, now: Instant, policy: &RestartPolicy) -> (u32, bool) {
        self.crashes.push_back(now);
        while self
            .crashes
            .front()
            .is_some_and(|first| now.duration_since(*first) > policy.crash_window)
        {
            self.crashes.pop_front();
        }
        let count = self.crashes.len();
        (
            u32::try_from(count).unwrap_or(u32::MAX),
            count > policy.crash_loop_threshold,
        )
    }

    /// Forgets past crashes (e.g. after a manual restart).
    pub fn reset(&mut self) {
        self.crashes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps_with_bounded_jitter() {
        let policy = RestartPolicy::default();
        for (attempt, base) in [(1, 1.0), (2, 2.0), (3, 4.0), (6, 32.0)] {
            let secs = policy.backoff(attempt).as_secs_f64();
            assert!(
                (base * 0.8..=base * 1.2).contains(&secs),
                "attempt {attempt}: {secs}"
            );
        }
        for attempt in [7, 20, u32::MAX] {
            assert!(policy.backoff(attempt) <= policy.max_backoff);
            assert!(policy.backoff(attempt) >= policy.max_backoff.mul_f64(0.8));
        }
    }

    #[test]
    fn detects_crash_loops_within_the_window() {
        let policy = RestartPolicy::default();
        let mut tracker = CrashTracker::default();
        let start = Instant::now();
        for i in 0..5 {
            let (attempt, looping) = tracker.record(start + Duration::from_secs(i * 10), &policy);
            assert_eq!(attempt, u32::try_from(i + 1).unwrap());
            assert!(!looping);
        }
        let (_, looping) = tracker.record(start + Duration::from_secs(60), &policy);
        assert!(looping, "6 crashes in 60 s is a loop");
    }

    #[test]
    fn old_crashes_expire() {
        let policy = RestartPolicy::default();
        let mut tracker = CrashTracker::default();
        let start = Instant::now();
        for i in 0..5 {
            tracker.record(start + Duration::from_secs(i), &policy);
        }
        let (attempt, looping) = tracker.record(start + Duration::from_secs(300), &policy);
        assert_eq!(attempt, 1);
        assert!(!looping);
    }
}
