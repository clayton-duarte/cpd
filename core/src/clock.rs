//! The time, as contract §1.3 writes it.

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
        todo!("stub: the Clock Job implements SystemClock::now")
    }
    fn sleep(&self, _d: Duration) {
        todo!("stub: the Clock Job implements SystemClock::sleep")
    }
}

/// Virtual time: `sleep` records the nap and advances `now()` by it. A clone shares the state.
#[derive(Clone)]
pub struct ManualClock {
    start: Instant,
}

impl ManualClock {
    pub fn new() -> Self {
        ManualClock { start: Instant::now() }
    }
    pub fn advance(&self, _d: Duration) {}
    pub fn elapsed(&self) -> Duration {
        Duration::MAX
    }
    pub fn naps(&self) -> Vec<Duration> {
        vec![Duration::MAX]
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        ManualClock::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start
    }
    fn sleep(&self, _d: Duration) {}
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
