//! Time source, so certificate lifetimes are testable without the real clock.

use std::{fmt, sync::Mutex};

use time::{Duration, OffsetDateTime};

/// Where "now" comes from.
pub trait Clock: fmt::Debug + Send + Sync {
    /// The current time, in UTC.
    fn now(&self) -> OffsetDateTime;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// A clock that only moves when told to (tests, simulations).
#[derive(Debug)]
pub struct ManualClock(Mutex<OffsetDateTime>);

impl ManualClock {
    /// A clock stopped at `at`.
    #[must_use]
    pub fn new(at: OffsetDateTime) -> Self {
        Self(Mutex::new(at))
    }

    /// Moves the clock forward (or back, with a negative duration).
    pub fn advance(&self, by: Duration) {
        let mut now = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *now += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> OffsetDateTime {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
