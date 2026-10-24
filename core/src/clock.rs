//! The time, as contract §1.3 writes it.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// `at` as contract §1.3 writes times: RFC 3339 UTC with milliseconds.
pub fn timestamp(at: OffsetDateTime) -> String {
    at.to_offset(UtcOffset::UTC)
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .expect("a UTC time always formats")
}

/// Now, as §1.3 writes times.
pub fn now() -> String {
    timestamp(OffsetDateTime::now_utc())
}

/// The time a wait reads and the nap it takes; a test passes a [`ManualClock`].
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
}

/// The real time: `Instant::now` and `thread::sleep`.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[derive(Default)]
struct State {
    elapsed: Duration,
    naps: Vec<Duration>,
}

/// Virtual time: `sleep` records the nap and advances `now()` by it. A clone shares the state.
#[derive(Clone)]
pub struct ManualClock {
    start: Instant,
    state: Arc<Mutex<State>>,
}

impl ManualClock {
    pub fn new() -> Self {
        ManualClock { start: Instant::now(), state: Arc::default() }
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("the clock's state is never held across a panic")
    }
    /// Moves virtual time forward without recording a nap.
    pub fn advance(&self, d: Duration) {
        self.state().elapsed += d;
    }
    /// The virtual time passed since construction.
    pub fn elapsed(&self) -> Duration {
        self.state().elapsed
    }
    /// Every `sleep`, in order.
    pub fn naps(&self) -> Vec<Duration> {
        self.state().naps.clone()
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        ManualClock::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start + self.elapsed()
    }
    fn sleep(&self, d: Duration) {
        let mut state = self.state();
        state.naps.push(d);
        state.elapsed += d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn a_time_is_written_in_utc_with_milliseconds() {
        assert_eq!(timestamp(datetime!(2026-05-01 12:34:56.789_123 +02:00)), "2026-05-01T10:34:56.789Z");
    }
}
